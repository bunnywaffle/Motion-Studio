//! Video footage decoding.
//!
//! The app has no library-level video decoder; instead it shells out to the
//! machine-local ffprobe/ffmpeg pair (the same tool the export pipeline
//! uses — `export::find_ffmpeg`) to describe the stream and to pull one
//! RGBA frame at a time. Decoding happens on the raster pass; repeated
//! playback of the same frame is served from the caller's small FIFO ring.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::RgbaImage;

/// Describe a video stream; `None` when ffprobe is missing or the file
/// cannot be parsed (e.g. the fake sample footage).
#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    /// Frame count at `fps` (0 when unknown).
    pub frames: i64,
}

/// Env override, then the sibling of whatever `export::find_ffmpeg` finds,
/// then a PATH search.
pub fn find_ffprobe() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("FFPROBE_PATH") {
        let b = PathBuf::from(v);
        if b.is_file() {
            return Some(b);
        }
    }
    if let Some(ff) = export::find_ffmpeg() {
        let probe = ff.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
        if probe.is_file() {
            return Some(probe);
        }
    }
    let n = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    std::env::var_os("PATH").and_then(|ps| {
        std::env::split_paths(&ps).find_map(|d| {
            let q = d.join(n);
            if q.is_file() {
                Some(q)
            } else {
                None
            }
        })
    })
}

fn parse_fps(s: &str) -> Option<f32> {
    let (num, den) = s.split_once('/')?;
    let n: f64 = num.parse().ok()?;
    let d: f64 = den.parse().ok()?;
    if d.abs() < 1e-9 {
        return None;
    }
    Some((n / d) as f32)
}

/// Probe a video file for stream metadata.
pub fn probe_video(path: &Path) -> Option<VideoInfo> {
    let ffprobe = find_ffprobe()?;
    let out = Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate,nb_frames:format=duration",
            "-of",
            "json",
            "-i",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let stream = v.get("streams")?.get(0)?;
    let width = stream.get("width")?.as_u64()? as u32;
    let height = stream.get("height")?.as_u64()? as u32;
    let fps = parse_fps(stream.get("avg_frame_rate")?.as_str()?)?;
    let frames = stream
        .get("nb_frames")
        .and_then(|n| n.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| {
            v.get("format")?
                .get("duration")?
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .map(|d| (d * fps as f64) as i64)
        })
        .unwrap_or(0);
    Some(VideoInfo {
        width,
        height,
        fps,
        frames,
    })
}

/// Longest-side cap for decoded frames (same 2048 as images).
pub(crate) const MAX_FRAME_SIDE: u32 = 2048;

/// Scaled frame dims (exactly what the decoder produces), so the layer box
/// and the pixels always agree.
pub(crate) fn scaled_video_dims(info: &VideoInfo) -> (u32, u32) {
    let longest = info.width.max(info.height) as f32;
    let scale = (MAX_FRAME_SIDE as f32 / longest).min(1.0);
    (
        ((info.width as f32 * scale).round() as u32).max(2),
        ((info.height as f32 * scale).round() as u32).max(2),
    )
}

/// Pull one video frame as an RGBA image (scaled to `MAX_FRAME_SIDE`).
pub fn decode_video_frame_at(path: &Path, frame: i64, info: &VideoInfo) -> Option<RgbaImage> {
    if frame < 0 {
        return None;
    }
    let ffmpeg = export::find_ffmpeg()?;
    let t = frame as f64 / info.fps.max(1e-3) as f64;
    let (dw, dh) = scaled_video_dims(info);
    let out = Command::new(&ffmpeg)
        .args(["-nostdin", "-v", "error"])
        .arg("-ss")
        .arg(format!("{t:.6}"))
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1"])
        .arg("-vf")
        .arg(format!("scale={dw}:{dh}"))
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    if out.stdout.len() != (dw as usize) * (dh as usize) * 4 {
        return None;
    }
    RgbaImage::from_raw(dw, dh, out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Skip when the test machine has no ffmpeg/ffprobe (the suite also runs
    /// on CI boxes without them).
    #[test]
    fn probe_and_decode_generated_clip() {
        let Some(ffmpeg) = export::find_ffmpeg() else {
            return;
        };
        if find_ffprobe().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "motion_video_decode_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mp4");
        let status = Command::new(&ffmpeg)
            .args([
                "-y",
                "-f", "lavfi",
                "-i",
                "testsrc=size=64x48:rate=10:duration=1",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&clip)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !status {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let info = probe_video(&clip).expect("probe parses generated clip");
        assert_eq!((info.width, info.height), (64, 48));
        assert!((info.fps - 10.0).abs() < 0.5);
        assert!(info.frames >= 8, "frames={}", info.frames);

        let img = decode_video_frame_at(&clip, 2, &info).expect("frame decodes");
        assert_eq!((img.width(), img.height()), (64, 48));
        // testsrc is always colorful; a fully transparent or all-zero frame
        // would mean the pipe was empty.
        assert!(img.pixels().any(|p| p[0] + p[1] + p[2] > 0));
        // Out-of-range frames are clamped by callers; the decoder itself
        // must not invent negatives.
        assert!(decode_video_frame_at(&clip, -1, &info).is_none());
        // Garbage input is rejected, not panicked on.
        let junk = dir.join("junk.mp4");
        std::fs::write(&junk, b"NOT_A_VIDEO").unwrap();
        assert!(probe_video(&junk).is_none());
        assert!(decode_video_frame_at(&junk, 0, &info).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
