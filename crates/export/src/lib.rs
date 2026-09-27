use project::{Composition, Project, TimeCode};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("unknown composition: {0}")]
    UnknownComposition(String),
    #[error("composition has no drawable area ({0}x{1})")]
    BadDimensions(u32, u32),
    #[error("frame {0} failed to render: {1}")]
    FrameFailed(i64, String),
    #[error("io error: {0}")]
    Io(String),
    #[error("image encode error: {0}")]
    Image(String),
    #[error("ffmpeg error: {0}")]
    Ffmpeg(String),
}
impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    #[default]
    PngSequence,
    PngStill,
    Gif,
    Mp4,
    WebM,
}
impl ExportFormat {
    pub fn all() -> &'static [ExportFormat] {
        &[
            Self::Mp4,
            Self::WebM,
            Self::Gif,
            Self::PngSequence,
            Self::PngStill,
        ]
    }
    /// Human label for the export dialog tiles.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4 (H.264)",
            Self::WebM => "WebM (VP9)",
            Self::Gif => "Animated GIF",
            Self::PngSequence => "PNG Sequence",
            Self::PngStill => "PNG Still",
        }
    }
    /// Short one-line hint shown under the format tiles.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Mp4 => "Best for delivery: small files, universal playback.",
            Self::WebM => "Open format for web delivery, no licence fees.",
            Self::Gif => "Loops anywhere, 256 colours, capped size.",
            Self::PngSequence => "Lossless frames in a numbered folder.",
            Self::PngStill => "A single frame at the current time.",
        }
    }
    /// True when the output is a muxed movie (needs ffmpeg).
    pub fn is_movie(self) -> bool {
        matches!(self, Self::Mp4 | Self::WebM)
    }
    /// True when frames are written as a numbered PNG folder first.
    pub fn writes_sequence(self) -> bool {
        matches!(self, Self::Mp4 | Self::WebM | Self::PngSequence)
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            _ => "png",
        }
    }
}
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub comp_id: String,
    pub format: ExportFormat,
    pub output: PathBuf,
    pub start_frame: Option<i64>,
    pub end_frame: Option<i64>,
    pub gif_max_side: u32,
    pub gif_fps: f64,
}
impl ExportJob {
    pub fn for_comp(a: impl Into<String>, b: ExportFormat, c: PathBuf) -> Self {
        Self { comp_id: a.into(), format: b, output: c,
            start_frame: None, end_frame: None, gif_max_side: 640, gif_fps: 15.0 }
    }
    pub fn range(&self, p: &Project) -> Result<(Composition, u32, u32, i64, i64, f64), ExportError> {
        let comp = p.get_composition(&self.comp_id).cloned()
            .ok_or_else(|| ExportError::UnknownComposition(self.comp_id.clone()))?;
        if comp.width == 0 || comp.height == 0 {
            return Err(ExportError::BadDimensions(comp.width, comp.height));
        }
        let total = comp.duration.frames().max(0);
        let mut s = self.start_frame.unwrap_or(0).max(0).min(total);
        let mut e = self.end_frame.unwrap_or(total).max(0).min(total);
        if s > e {
            std::mem::swap(&mut s, &mut e);
        }
        let w = comp.width;
        let h = comp.height;
        let fps = if comp.frame_rate > 0.0 { comp.frame_rate } else { 30.0 };
        Ok((comp, w, h, s, e, fps))
    }
}
/// Progress callback: `(frames_done, frames_total)`. Invoked from the render
/// thread, so implementations must be thread-safe — the UI reaches for an
/// atomic counter here rather than touching widget state directly.
pub type ProgressFn<'a> = dyn Fn(usize, usize) + Send + Sync + 'a;
#[derive(Debug, Clone)]
pub struct ExportResult {
    pub primary: PathBuf,
    pub frames: usize,
    pub note: Option<String>,
    pub fallback: Option<PathBuf>,
}
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("FFMPEG_PATH") {
        let b = PathBuf::from(v);
        if b.is_file() { return Some(b); }
    }
    let n = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    std::env::var_os("PATH").and_then(|ps| {
        std::env::split_paths(&ps).find_map(|d| {
            let q = d.join(n);
            if q.is_file() { Some(q) } else { None }
        })
    })
}
fn png(p: &Path, w: u32, h: u32, b: &[u8]) -> Result<(), ExportError> {
    if let Some(par) = p.parent() {
        if !par.as_os_str().is_empty() { std::fs::create_dir_all(par)?; }
    }
    image::save_buffer(p, b, w, h, image::ColorType::Rgba8).map_err(|e| ExportError::Image(e.to_string()))
}
fn stem_of(j: &ExportJob) -> String {
    j.output.file_stem().and_then(|s| s.to_str()).unwrap_or("frame").to_string()
}
fn seqdir_of(j: &ExportJob) -> PathBuf {
    let p = &j.output;
    if p.exists() && p.is_dir() {
        return p.clone();
    }
    let em = p.extension().and_then(|x| x.to_str()).unwrap_or("zz").to_lowercase();
    if em == "mp4" || em == "webm" || em == "gif" || em == "png" {
        let st = stem_of(j);
        if let Some(par) = p.parent() {
            if !par.as_os_str().is_empty() {
                return par.join(st + "_seq");
            }
        }
        return PathBuf::from(st + "_seq");
    }
    p.clone()
}
fn withext_of(p: &Path, e: &str) -> PathBuf {
    match p.extension().and_then(|x| x.to_str()) {
        Some(c) if c.eq_ignore_ascii_case(e) => p.to_path_buf(),
        _ => p.with_extension(e),
    }
}
fn mux_one(ff: &Path, dir: &Path, st: &str, fps: f64, mv: &Path, webm: bool) -> Result<(), ExportError> {
    let pat = dir.join(st.to_owned() + "_%05d.png");
    let rate = if fps > 0.0 { fps } else { 30.0 };
    let mut c = std::process::Command::new(ff);
    c.arg("-y")
        .arg("-framerate")
        .arg(format!("{rate}"))
        .arg("-start_number")
        .arg("1")
        .arg("-i")
        .arg(&pat);
    if webm {
        c.args(["-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p"]);
    } else {
        c.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18"]);
    }
    c.arg(mv);
    let o = c.output().map_err(|e| ExportError::Ffmpeg(e.to_string()))?;
    if !o.status.success() {
        return Err(ExportError::Ffmpeg(
            String::from_utf8_lossy(&o.stderr).trim().to_string(),
        ));
    }
    Ok(())
}
pub fn render_job(
    p: &Project,
    j: &ExportJob,
    f: impl Fn(i64, TimeCode, u32, u32) -> Result<Vec<u8>, String>,
    pg: Option<&ProgressFn<'_>>,
) -> Result<ExportResult, ExportError> {
    let (cp, w, h, s, e, fps) = j.range(p)?;
    let total = (e - s + 1).max(1) as usize;
    let rep = |d: usize| { if let Some(c) = pg { c(d, total); } };
    if j.format == ExportFormat::PngStill {
        let tc = TimeCode::from_frames(s, cp.frame_rate);
        let b = f(s, tc, w, h).map_err(|x| ExportError::FrameFailed(s, x))?;
        png(&j.output, w, h, &b)?;
        rep(total);
        return Ok(ExportResult { primary: j.output.clone(), frames: 1, note: None, fallback: None });
    }
    if j.format == ExportFormat::Gif {
        let mx = j.gif_max_side.clamp(64, 2048).max(1);
        let sc = (mx as f64 / w.max(h).max(1) as f64).min(1.0);
        let gw = ((w as f64 * sc).round() as u32).max(1);
        let gh = ((h as f64 * sc).round() as u32).max(1);
        let want = if j.gif_fps > 0.0 { j.gif_fps } else { 15.0 };
        let step = ((fps / want).round() as i64).max(1);
        let path = withext_of(&j.output, "gif");
        if let Some(par) = path.parent() { if !par.as_os_str().is_empty() { std::fs::create_dir_all(par)?; } }
        let file = std::fs::File::create(&path)?;
        let mut enc = image::codecs::gif::GifEncoder::new_with_speed(file, 10);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).map_err(|x| ExportError::Image(x.to_string()))?;
        let mut done = 0usize;
        let mut fr = s;
        while fr <= e {
            let tc = TimeCode::from_frames(fr, cp.frame_rate);
            let b = f(fr, tc, w, h).map_err(|x| ExportError::FrameFailed(fr, x))?;
            let sm: Vec<u8> = if gw != w || gh != h {
                let mut o = vec![0u8; gw as usize * gh as usize * 4];
                let mut yy = 0u32;
                while yy < gh {
                    let sy = (yy as u64 * h as u64 / gh as u64).min(h as u64 - 1) as u32;
                    let mut xx = 0u32;
                    while xx < gw {
                        let sx = (xx as u64 * w as u64 / gw as u64).min(w as u64 - 1) as u32;
                        let si = ((sy * w + sx) * 4) as usize;
                        let di = ((yy * gw + xx) * 4) as usize;
                        o[di..di + 4].copy_from_slice(&b[si..si + 4]);
                        xx += 1;
                    }
                    yy += 1;
                }
                o
            } else {
                b.clone()
            };
            let img = image::RgbaImage::from_raw(gw, gh, sm).ok_or_else(|| ExportError::Image("gif".into()))?;
            let dl = ((step.max(1) as f64 / fps.max(1.0) * 100.0).round().clamp(2.0, 200.0)) as u32;
            let fr2 = image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(dl * 10, 1));
            enc.encode_frame(fr2).map_err(|x| ExportError::Image(x.to_string()))?;
            done += 1;
            rep(done);
            fr += step.max(1);
        }
        return Ok(ExportResult { primary: path, frames: done, note: None, fallback: None });
    }
    let dir = seqdir_of(j);
    std::fs::create_dir_all(&dir)?;
    let st = stem_of(j);
    let mut done = 0usize;
    let mut fr = s;
    while fr <= e {
        let tc = TimeCode::from_frames(fr, cp.frame_rate);
        let b = f(fr, tc, w, h).map_err(|x| ExportError::FrameFailed(fr, x))?;
        let n = done + 1;
        let name = st.clone() + "_" + &format!("{:05}", n) + ".png";
        png(&dir.join(name), w, h, &b)?;
        done = n;
        rep(done);
        fr += 1;
    }
    if j.format == ExportFormat::PngSequence {
        return Ok(ExportResult { primary: dir, frames: done, note: None, fallback: None });
    }
    let mv = withext_of(&j.output, j.format.extension());
    if let Some(par) = mv.parent() { if !par.as_os_str().is_empty() { std::fs::create_dir_all(par)?; } }
    match find_ffmpeg() {
        Some(ff) => {
            let webm = j.format == ExportFormat::WebM;
            mux_one(&ff, &dir, &st, fps, &mv, webm)?;
            Ok(ExportResult { primary: mv, frames: done, note: None, fallback: None })
        }
        None => Ok(ExportResult { primary: dir.clone(), frames: done,
            note: Some("ffmpeg missing".into()), fallback: Some(dir) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use project::{Color, Composition};

    /// A tiny 64x48 / 30fps project whose "frame" is a solid colour ramp so
    /// every frame is visually distinct and trivially verifiable.
    fn test_project(frames: i64) -> Project {
        let mut p = Project::with_defaults("Export Test");
        let mut c = Composition::new(
            "comp_export",
            "Export Test Comp",
            64,
            48,
            30.0,
            TimeCode::from_frames(frames, 30.0),
        );
        c.background_color = Color::rgba(0.1, 0.2, 0.3, 1.0);
        p.compositions.push(c);
        p
    }

    /// Synthetic renderer: fills the buffer with a frame-dependent colour.
    fn frame_fn(fr: i64, _tc: TimeCode, w: u32, h: u32) -> Result<Vec<u8>, String> {
        let v = (fr * 10).clamp(0, 255) as u8;
        let mut b = vec![0u8; w as usize * h as usize * 4];
        for i in (0..b.len()).step_by(4) {
            b[i] = v;
            b[i + 1] = 32;
            b[i + 2] = 64;
            b[i + 3] = 255;
        }
        Ok(b)
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("motion_export_{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn format_metadata_is_consistent() {
        for f in ExportFormat::all() {
            assert!(!f.label().is_empty());
            assert!(!f.hint().is_empty());
            assert!(!f.extension().is_empty());
        }
        assert_eq!(ExportFormat::all().len(), 5);
        assert_eq!(ExportFormat::Mp4.extension(), "mp4");
        assert_eq!(ExportFormat::WebM.extension(), "webm");
        assert_eq!(ExportFormat::Gif.extension(), "gif");
        assert_eq!(ExportFormat::PngStill.extension(), "png");
        assert!(ExportFormat::Mp4.is_movie());
        assert!(ExportFormat::WebM.is_movie());
        assert!(!ExportFormat::Gif.is_movie());
        assert!(ExportFormat::PngSequence.writes_sequence());
        assert!(!ExportFormat::PngStill.writes_sequence());
        assert_eq!(ExportFormat::default(), ExportFormat::PngSequence);
    }

    #[test]
    fn job_range_clamps_to_composition_duration() {
        let p = test_project(10);
        let mut job = ExportJob::for_comp("comp_export", ExportFormat::Mp4, PathBuf::from("o.mp4"));
        let (_, w, h, s, e, fps) = job.range(&p).expect("range");
        assert_eq!((w, h), (64, 48));
        assert_eq!((s, e), (0, 10));
        assert!((fps - 30.0).abs() < 1e-9);

        // Explicit, out-of-bounds range is clamped and never inverted.
        job.start_frame = Some(7);
        job.end_frame = Some(99);
        let (_, _, _, s, e, _) = job.range(&p).expect("range");
        assert_eq!((s, e), (7, 10));

        job.start_frame = Some(9);
        job.end_frame = Some(2);
        let (_, _, _, s, e, _) = job.range(&p).expect("range");
        assert!(s <= e, "start {s} must not exceed end {e}");

        // Unknown composition is a hard error, not a silent empty render.
        let bad = ExportJob::for_comp("nope", ExportFormat::PngStill, PathBuf::from("x.png"));
        assert!(matches!(
            bad.range(&p),
            Err(ExportError::UnknownComposition(_))
        ));
    }

    #[test]
    fn path_helpers_pick_sequence_dirs_and_extensions() {
        let j = ExportJob::for_comp(
            "comp_export",
            ExportFormat::Mp4,
            PathBuf::from("out/movie.mp4"),
        );
        assert_eq!(stem_of(&j), "movie");
        assert_eq!(seqdir_of(&j), PathBuf::from("out/movie_seq"));
        assert_eq!(
            withext_of(Path::new("a/b.mp4"), "mp4"),
            PathBuf::from("a/b.mp4")
        );
        assert_eq!(
            withext_of(Path::new("a/b.mov"), "mp4"),
            PathBuf::from("a/b.mp4")
        );
        assert_eq!(withext_of(Path::new("a/b"), "gif"), PathBuf::from("a/b.gif"));
    }

    #[test]
    fn renders_png_still_at_the_requested_frame() {
        let p = test_project(10);
        let dir = tmp_dir("still");
        let out = dir.join("still.png");
        let job = ExportJob {
            comp_id: "comp_export".into(),
            format: ExportFormat::PngStill,
            output: out.clone(),
            start_frame: Some(4),
            end_frame: Some(4),
            gif_max_side: 640,
            gif_fps: 15.0,
        };
        let res = render_job(&p, &job, frame_fn, None).expect("still render");
        assert_eq!(res.frames, 1);
        assert_eq!(res.primary, out);
        let img = image::open(&out).expect("decode still png");
        assert_eq!((img.width(), img.height()), (64, 48));
        // Frame 4 => red channel 40 via `frame_fn`.
        let px = img.to_rgba8().get_pixel(0, 0).0;
        assert_eq!(px, [40, 32, 64, 255]);
    }

    #[test]
    fn renders_png_sequence_with_one_based_names() {
        let p = test_project(10);
        let dir = tmp_dir("seq");
        let out = dir.join("clip");
        let job = ExportJob {
            comp_id: "comp_export".into(),
            format: ExportFormat::PngSequence,
            output: out.clone(),
            start_frame: Some(0),
            end_frame: Some(3),
            gif_max_side: 640,
            gif_fps: 15.0,
        };
        let res = render_job(&p, &job, frame_fn, None).expect("sequence render");
        assert_eq!(res.frames, 4);
        assert_eq!(res.primary, out);
        let mut names: Vec<String> = std::fs::read_dir(&out)
            .expect("seq dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 4);
        assert!(names[0].ends_with("_00001.png"), "got {names:?}");
        assert!(names[3].ends_with("_00004.png"), "got {names:?}");
    }

    #[test]
    fn renders_animated_gif_and_reports_progress() {
        let p = test_project(10);
        let dir = tmp_dir("gif");
        let out = dir.join("anim.gif");
        let job = ExportJob {
            comp_id: "comp_export".into(),
            format: ExportFormat::Gif,
            output: out.clone(),
            start_frame: Some(0),
            end_frame: Some(9),
            gif_max_side: 64,
            gif_fps: 15.0,
        };
        // 30fps source at 15fps target => every other frame, 5 frames total.
        let seen = std::sync::Mutex::new(Vec::new());
        let report = |done: usize, total: usize| {
            seen.lock().unwrap().push((done, total));
        };
        let res = render_job(&p, &job, frame_fn, Some(&report)).expect("gif render");
        assert_eq!(res.frames, 5);
        assert!(res.primary.exists(), "gif written");
        assert!(std::fs::metadata(&res.primary).unwrap().len() > 0);
        let log = seen.lock().unwrap();
        assert!(!log.is_empty(), "progress callback fired");
        assert_eq!(log.last().unwrap().0, 5, "final progress is the frame count");
        assert!(log.windows(2).all(|w| w[0].0 <= w[1].0), "progress is monotonic");
    }

    #[test]
    fn missing_ffmpeg_falls_back_to_a_png_sequence() {
        let p = test_project(2);
        let dir = tmp_dir("fallback");
        let out = dir.join("movie.mp4");
        let job = ExportJob {
            comp_id: "comp_export".into(),
            format: ExportFormat::Mp4,
            output: out,
            start_frame: Some(0),
            end_frame: Some(1),
            gif_max_side: 640,
            gif_fps: 15.0,
        };
        // Whatever ffmpeg availability this machine has, the call must succeed:
        // with a muxer it writes the movie, without one it degrades to a folder.
        let res = render_job(&p, &job, frame_fn, None).expect("mp4 render");
        assert_eq!(res.frames, 2);
        if find_ffmpeg().is_some() {
            let mv = &res.primary;
            assert_eq!(mv.extension().and_then(|e| e.to_str()), Some("mp4"));
            let bytes = std::fs::read(mv).expect("read muxed movie");
            assert!(bytes.len() > 512, "muxed movie is not empty");
            assert_eq!(&bytes[4..8], b"ftyp", "output is a real MP4 container");
        } else {
            assert_eq!(res.note.as_deref(), Some("ffmpeg missing"));
            assert!(res.primary.is_dir(), "falls back to the frame folder");
        }
    }
}
