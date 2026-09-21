//! Runtime user-shader ("Shader Lab") pipeline.
//!
//! Users write effects in a documented GLSL-style subset:
//!
//! ```glsl
//! // @param name="Brightness" type=float min=-2 max=2 step=0.01
//! uniform float brightness = 0.0;
//!
//! vec4 effect(vec2 uv, vec4 color) {
//!     return color + vec4(brightness);
//! }
//! ```
//!
//! [`compile_source`] transpiles this to WGSL, wraps it with a fullscreen
//! vertex stage, an input-texture sampler, and a `Params` uniform block
//! (`time`, `resolution`, `frame`, `duration` + user uniforms), then
//! validates it with `naga`. A failing source returns a human-readable
//! error so the caller can keep the last-good shader active.
//!
//! Supported subset (anything else fails validation with a clear error):
//! entry `vec4 effect(vec2, vec4)` on one line, typed helper functions,
//! `float/int/uint/bool/vec2/vec3/vec4` types and constructors, swizzles,
//! built-in math (`mix`, `clamp`, `smoothstep`, `sin`, ...). No
//! preprocessor (except `#version`/`precision`, which are ignored), no
//! ternary `?:` (use `if` or `mix`), no `main()`.

use crate::device::{GpuContext, GpuError, RenderTarget};
use project::{ShaderParam, ShaderParamValue};
use std::collections::HashMap;

/// A uniform block member with its byte offset in the upload buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct UniformField {
    pub name: String,
    pub wgsl_type: &'static str,
    pub offset: usize,
    pub size: usize,
}

/// Validated + transpiled shader ready for pipeline creation / upload.
#[derive(Debug, Clone)]
pub struct CachedShader {
    pub wgsl: String,
    pub fields: Vec<UniformField>,
    pub buffer_size: u64,
    /// User uniform names in declaration order.
    pub param_names: Vec<String>,
}

fn align_up(offset: usize, align: usize) -> usize {
    offset.div_ceil(align) * align
}

fn wgsl_type_and_layout(param: &ShaderParam) -> (&'static str, usize, usize) {
    use project::ShaderParamType::*;
    match param.param_type {
        Float | Angle => ("f32", 4, 4),
        Int | Bool | Enum { .. } => ("i32", 4, 4),
        Vec2 => ("vec2<f32>", 8, 8),
        Vec3 => ("vec3<f32>", 16, 12),
        Vec4 | Color => ("vec4<f32>", 16, 16),
    }
}

/// Runtime-provided uniforms (always present in the `Params` block).
pub const RUNTIME_UNIFORMS: &[&str] = &["time", "resolution", "frame", "duration"];

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Replace whole-word tokens per `map`, skipping member access after `.`
/// and already-generic `name<...>` occurrences.
fn map_tokens(code: &str, map: &HashMap<&str, &str>) -> String {
    let mut out = String::with_capacity(code.len());
    let chars: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && is_ident_char(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // Lookahead / lookbehind.
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_whitespace() {
                j += 1;
            }
            let preceded_by_dot = {
                let mut k = start;
                while k > 0 && chars[k - 1].is_ascii_whitespace() {
                    k -= 1;
                }
                k > 0 && chars[k - 1] == '.'
            };
            if !preceded_by_dot && j < chars.len() && chars[j] == '<' {
                out.push_str(&word); // already generic: vec2<f32>, etc.
            } else if !preceded_by_dot {
                if let Some(rep) = map.get(word.as_str()) {
                    out.push_str(rep);
                } else {
                    out.push_str(&word);
                }
            } else {
                out.push_str(&word);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Insert WGSL `let`/`var` bindings for GLSL-style declarations.
///
/// After type mapping, a trimmed line like `vec3<f32> c = ...;` or
/// `f32 x;` becomes `let c = ...;`. `for` initializers become
/// `var i: i32 = 0`, and `++`/`--` become `+= 1`/`-= 1`. Multi-declarator
/// lines (`float a = 1.0, b = 2.0;`) are rejected with guidance.
fn insert_bindings(line: &str) -> Result<String, String> {
    // `++` / `--` first (WGSL spells them `+= 1` / `-= 1`).
    let line = line.replace("++", "+= 1").replace("--", "-= 1");
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with('@')
        || trimmed.starts_with("fn ")
        || trimmed.starts_with("struct ")
        || trimmed == "{"
        || trimmed == "}"
    {
        return Ok(line);
    }
    for kw in [
        "if", "for", "while", "loop", "return", "else", "switch", "case", "default",
        "break", "continue", "discard", "const_assert",
    ] {
        if trimmed == kw
            || trimmed.starts_with(&format!("{kw} "))
            || trimmed.starts_with(&format!("{kw}("))
            || trimmed.starts_with(&format!("{kw}{{"))
        {
            if kw == "for" {
                return rewrite_for_loop(&line);
            }
            return Ok(line);
        }
    }
    // Strip a leading `const` (WGSL `let` covers locals).
    let line = match trimmed.strip_prefix("const ") {
        Some(rest) => {
            let indent: String = line.chars().take_while(|c| c.is_ascii_whitespace()).collect();
            format!("{indent}{rest}")
        }
        None => line,
    };
    let code = strip_line_comment(line.trim()).trim().to_string();
    // Declaration? `TYPE NAME [= ...] ;` with no parens before `=`/`;`.
    let end = code.find(['=', ';']).unwrap_or(code.len());
    let head = code[..end].trim();
    if head.contains('(') || head.contains('.') {
        return Ok(line);
    }
    let toks: Vec<&str> = head.split_whitespace().collect();
    let wgsl_types = [
        "f32", "i32", "u32", "bool", "vec2<f32>", "vec3<f32>", "vec4<f32>",
    ];
    if toks.len() == 2 && wgsl_types.contains(&toks[0]) && toks[1].chars().all(is_ident_char) {
        // Reject multi-declarators (`float a = 1.0, b = 2.0;`): a top-level
        // comma past the declarator. Commas inside constructor calls sit at
        // paren depth > 0 and are fine.
        let mut depth = 0;
        for c in code[head.len()..].chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    return Err(format!(
                        "one declaration per line please (split `{}`)",
                        code.trim().trim_end_matches(';')
                    ));
                }
                _ => {}
            }
        }
        let indent: String = line.chars().take_while(|c| c.is_ascii_whitespace()).collect();
        let rest = line.trim_start()[head.len()..].to_string();
        // GLSL locals are mutable, so everything becomes `var` (naga rejects
        // reassignment of `let` bindings).
        return Ok(format!("{indent}var {}: {}{rest}", toks[1], toks[0]));
    }
    Ok(line)
}

/// Rewrite `for (TYPE i = v; cond; incr)` initializers to WGSL `var` form.
fn rewrite_for_loop(line: &str) -> Result<String, String> {
    let open = line.find('(').unwrap_or(0);
    let mut depth = 0;
    let mut close = line.len();
    for (i, c) in line.char_indices() {
        if i < open {
            continue;
        }
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                close = i;
                break;
            }
        }
    }
    let inner = &line[open + 1..close];
    let mut parts = inner.splitn(2, ';');
    let init = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("");
    let toks: Vec<&str> = init.split_whitespace().collect();
    let wgsl_types = [
        "f32", "i32", "u32", "bool", "vec2<f32>", "vec3<f32>", "vec4<f32>",
    ];
    let new_init = if toks.len() >= 3 && wgsl_types.contains(&toks[0]) && toks[2] == "=" {
        format!("var {}: {} = {}", toks[1], toks[0], toks[3..].join(" "))
    } else {
        return Err(format!(
            "unsupported `for` initializer `{init}`; use `for (int i = 0; ...)`"
        ));
    };
    Ok(format!("{}({};{})){}", &line[..open], new_init, rest, &line[close + 1..]))
}

/// Translate one helper signature `ret name(args)` into WGSL `fn` syntax.
/// Returns `None` when the line is not a function signature.
fn translate_signature(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.contains('(') || !line.contains(')') {
        return None;
    }
    // Skip control flow / builtins that merely look similar.
    for kw in ["if", "for", "while", "switch", "return"] {
        if line.starts_with(kw) && line[kw.len()..].starts_with([' ', '\t', '(']) {
            return None;
        }
    }
    let paren = line.find('(')?;
    let head = line[..paren].trim();    let mut head_parts: Vec<&str> = head.split_whitespace().collect();
    if head_parts.len() < 2 {
        return None;
    }
    let name = head_parts.pop()?;
    if !name.chars().all(is_ident_char) {
        return None;
    }
    let ret = head_parts.pop()?;
    if head_parts.iter().any(|p| *p != "const" && *p != "in") {
        return None;
    }
    // Depth-matched close paren (one-liner bodies contain more parens).
    let mut depth = 0;
    let mut close = None;
    for (i, c) in line.char_indices() {
        if i < paren {
            continue;
        }
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                close = Some(i);
                break;
            }
        }
    }
    let close = close?;
    let args_raw = &line[paren + 1..close];
    let tail = line[close + 1..].trim().trim_end_matches('{').trim().to_string();
    let mut args = Vec::new();
    if !args_raw.trim().is_empty() {
        for arg in args_raw.split(',') {
            let arg = arg.trim();
            let toks: Vec<&str> = arg.split_whitespace().collect();
            if toks.len() < 2 {
                return None;
            }
            let aname = toks[toks.len() - 1];
            let aty = toks[toks.len() - 2];
            if !aname.chars().all(is_ident_char) {
                return None;
            }
            let map: HashMap<&str, &str> = [
                ("float", "f32"),
                ("int", "i32"),
                ("uint", "u32"),
                ("bool", "bool"),
                ("vec2", "vec2<f32>"),
                ("vec3", "vec3<f32>"),
                ("vec4", "vec4<f32>"),
            ]
            .into_iter()
            .collect();
            let wty = map.get(aty).copied().unwrap_or(aty);
            args.push(format!("{aname}: {wty}"));
        }
    }
    let ret_map: HashMap<&str, &str> = [
        ("float", "f32"),
        ("int", "i32"),
        ("uint", "u32"),
        ("bool", "bool"),
        ("vec2", "vec2<f32>"),
        ("vec3", "vec3<f32>"),
        ("vec4", "vec4<f32>"),
    ]
    .into_iter()
    .collect();
    let ret_wgsl = if ret == "void" {
        String::new()
    } else {
        format!(" -> {}", ret_map.get(ret).copied().unwrap_or(ret))
    };
    let brace = if tail.is_empty() && line.trim_end().ends_with('{') {
        " {"
    } else if tail.is_empty() {
        ""
    } else {
        &format!(" {tail}")
    };
    Some(format!("fn {name}({}){ret_wgsl}{brace}", args.join(", ")))
}

/// Transpile Shader Lab source into a complete WGSL module.
/// Returns the validated shader plus the ordered user parameter definitions.
fn transpile_to_wgsl(source: &str) -> Result<(CachedShader, Vec<ShaderParam>), String> {
    let params = project::parse_shader_params(source);
    let uniform_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

    let mut body_lines: Vec<String> = Vec::new();
    let mut effect_found = false;
    let mut effect_uv = "uv".to_string();
    let mut effect_color = "color".to_string();

    for (lineno, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('#') {
            let directive = line[1..].trim_start();
            if directive.starts_with("version") || directive.starts_with("precision") {
                continue; // meaningless in WGSL, safe to drop.
            }
            return Err(format!(
                "line {}: preprocessor directives are not supported (found `{line}`)",
                lineno + 1
            ));
        }
        if line.starts_with("uniform") && line.contains(';') {
            continue; // declarations become wrapper members.
        }
        let code = strip_line_comment(line);
        if code.contains('?') {
            return Err(format!(
                "line {}: ternary `?:` is not supported; use `if` or `mix` instead",
                lineno + 1
            ));
        }
        // Entry point (must sit on one line).
        if code.contains("effect") && code.contains('(') && !effect_found {
            if let Some(sig) = translate_signature(code) {
                // Must be `vec4 effect(vec2, vec4)`; anything else is rejected.
                let head_ok = code.trim_start().starts_with("vec4")
                    && code.contains("vec2")
                    && code.contains("vec4");
                if sig.starts_with("fn effect(") && head_ok {
                    // Capture entry-local names so they are not `u.`-qualified.
                    if let (Some(lp), Some(rp)) = (code.find('('), code.rfind(')')) {
                        let args: Vec<&str> = code[lp + 1..rp].split(',').collect();
                        if args.len() == 2 {
                            let a0: Vec<&str> = args[0].split_whitespace().collect();
                            let a1: Vec<&str> = args[1].split_whitespace().collect();
                            if let (Some(u), Some(c)) = (a0.last(), a1.last()) {
                                effect_uv = u.to_string();
                                effect_color = c.to_string();
                            }
                        }
                    }
                    body_lines.push(sig);
                    effect_found = true;
                    continue;
                }
            }
        }
        // Typed helper functions (not `main`).
        if let Some(sig) = translate_signature(code) {
            if sig.starts_with("fn main(") || sig == "fn main()" {
                return Err(format!(
                    "line {}: `main()` is not used; write `vec4 effect(vec2 uv, vec4 color)` instead",
                    lineno + 1
                ));
            }
            if sig.starts_with("fn ") {
                body_lines.push(sig);
                continue;
            }
        }
        body_lines.push(raw.to_string());
    }

    if !effect_found {
        return Err(
            "missing entry point: define `vec4 effect(vec2 uv, vec4 color)` on a single line".to_string(),
        );
    }

    let mut body = body_lines.join("\n");
    // GLSL type keywords -> WGSL.
    let type_map: HashMap<&str, &str> = [
        ("float", "f32"),
        ("int", "i32"),
        ("uint", "u32"),
        ("bool", "bool"),
        ("vec2", "vec2<f32>"),
        ("vec3", "vec3<f32>"),
        ("vec4", "vec4<f32>"),
    ]
    .into_iter()
    .collect();
    body = map_tokens(&body, &type_map);
    // WGSL bindings: `vec3 c = ...` -> `let c = ...`.
    let mut bound_lines = Vec::with_capacity(body.lines().count());
    for (idx, line) in body.lines().enumerate() {
        bound_lines.push(insert_bindings(line).map_err(|e| format!("line {}: {e}", idx + 1))?);
    }
    body = bound_lines.join("\n");
    // Qualify uniforms (user + runtime) with the Params block, except entry
    // locals and member accesses.
    let runtime: Vec<String> = RUNTIME_UNIFORMS.iter().map(|s| s.to_string()).collect();
    let mut qual: HashMap<&str, String> = HashMap::new();
    for name in uniform_names.iter().chain(runtime.iter()) {
        if name == &effect_uv || name == &effect_color {
            continue;
        }
        qual.insert(name.as_str(), format!("u.{name}"));
    }
    let qual_ref: HashMap<&str, &str> =
        qual.iter().map(|(k, v)| (*k, v.as_str())).collect();
    body = map_tokens(&body, &qual_ref);

    // Uniform block layout: runtime header then user members with WGSL
    // alignment (vec3 aligns to 16). Total rounded up to 16.
    let mut fields: Vec<UniformField> = vec![
        UniformField { name: "time".into(), wgsl_type: "f32", offset: 0, size: 4 },
        UniformField { name: "resolution".into(), wgsl_type: "vec2<f32>", offset: 8, size: 8 },
        UniformField { name: "frame".into(), wgsl_type: "f32", offset: 16, size: 4 },
        UniformField { name: "duration".into(), wgsl_type: "f32", offset: 20, size: 4 },
    ];
    let mut offset = 24usize;
    let mut members = String::from(
        "    time: f32,\n    resolution: vec2<f32>,\n    frame: f32,\n    duration: f32,\n",
    );
    for p in &params {
        let (wty, align, size) = wgsl_type_and_layout(p);
        offset = align_up(offset, align);
        fields.push(UniformField { name: p.name.clone(), wgsl_type: wty, offset, size });
        members.push_str(&format!("    {}: {wty},\n", p.name));
        offset += size;
    }
    let total = align_up(offset, 16);
    // Trailing padding to satisfy the 16-byte multiple.
    let mut pad = offset;
    let mut pad_idx = 0;
    while pad < total {
        members.push_str(&format!("    _pad{pad_idx}: f32,\n"));
        pad += 4;
        pad_idx += 1;
    }

    let wgsl = format!(
        r#"struct Params {{
{members}}};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: Params;

struct VsOut {{
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {{
    var out: VsOut;
    let x = f32(i32(vi) / 2) * 4.0 - 1.0;
    let y = f32(i32(vi) % 2) * 4.0 - 1.0;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}}

{body}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {{
    let base = textureSample(src_tex, src_sampler, in.uv);
    return effect(in.uv, base);
}}
"#
    );

    // Validate with naga for precise, span-annotated errors.
    if let Err(err) = naga::front::wgsl::parse_str(&wgsl) {
        return Err(format!("shader error: {}", err.emit_to_string(&wgsl)));
    }

    let cached = CachedShader {
        wgsl,
        fields,
        buffer_size: total as u64,
        param_names: params.iter().map(|p| p.name.clone()).collect(),
    };
    Ok((cached, params))
}

/// Transpile + validate. Errors are human-readable (naga spans).
pub fn compile_source(source: &str) -> Result<(CachedShader, Vec<ShaderParam>), String> {
    transpile_to_wgsl(source)
}

/// Hash a source string for pipeline caching.
pub fn hash_source(source: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

/// Cache of validated shaders keyed by source hash: pipelines are compiled
/// once per unique source and reused across frames and layers.
#[derive(Debug, Default)]
pub struct ShaderLabCache {
    entries: HashMap<u64, CachedShader>,
}

impl ShaderLabCache {
    pub fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    /// Return the cached validation or compile it now. Failed sources are
    /// never inserted, so the previous working shader stays active.
    pub fn get_or_compile(&mut self, source: &str) -> Result<&CachedShader, String> {
        let key = hash_source(source);
        if !self.entries.contains_key(&key) {
            let (cached, _) = compile_source(source)?;
            self.entries.insert(key, cached);
        }
        Ok(&self.entries[&key])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Serialize one resolved value into the upload buffer at `field`.
fn write_field(buf: &mut [u8], field: &UniformField, value: &ShaderParamValue) {
    fn put(buf: &mut [u8], offset: usize, bytes: [u8; 4]) {
        if offset + 4 <= buf.len() {
            buf[offset..offset + 4].copy_from_slice(&bytes);
        }
    }
    let base = field.offset;
    match value {
        ShaderParamValue::Float(v) => put(buf, base, v.to_le_bytes()),
        ShaderParamValue::Int(v) => put(buf, base, v.to_le_bytes()),
        ShaderParamValue::Bool(v) => put(buf, base, (*v as i32).to_le_bytes()),
        ShaderParamValue::Vec2(v) => {
            put(buf, base, v[0].to_le_bytes());
            put(buf, base + 4, v[1].to_le_bytes());
        }
        ShaderParamValue::Vec3(v) => {
            put(buf, base, v[0].to_le_bytes());
            put(buf, base + 4, v[1].to_le_bytes());
            put(buf, base + 8, v[2].to_le_bytes());
        }
        ShaderParamValue::Vec4(v) => {
            for (i, c) in v.iter().enumerate() {
                put(buf, base + i * 4, c.to_le_bytes());
            }
        }
        ShaderParamValue::Color(c) => {
            put(buf, base, c.r.to_le_bytes());
            put(buf, base + 4, c.g.to_le_bytes());
            put(buf, base + 8, c.b.to_le_bytes());
            put(buf, base + 12, c.a.to_le_bytes());
        }
    }
}

/// Build the uniform upload buffer for one frame.
pub fn build_uniform_buffer(
    cached: &CachedShader,
    values: &HashMap<String, ShaderParamValue>,
    defaults: &HashMap<String, ShaderParamValue>,
    time: f32,
    resolution: (f32, f32),
    frame: f32,
    duration: f32,
) -> Vec<u8> {
    let mut buf = vec![0u8; cached.buffer_size as usize];
    let mut put = |offset: usize, v: f32| {
        if offset + 4 <= buf.len() {
            buf[offset..offset + 4].copy_from_slice(&v.to_le_bytes());
        }
    };
    put(0, time);
    put(8, resolution.0);
    put(12, resolution.1);
    put(16, frame);
    put(20, duration);
    for field in cached.fields.iter().skip(4) {
        if let Some(v) = values.get(&field.name).or_else(|| defaults.get(&field.name)) {
            write_field(&mut buf, field, v);
        }
    }
    buf
}

/// A GPU pipeline for one validated Shader Lab source, rendering the effect
/// over an input texture into a target.
pub struct ShaderLabPipeline {
    pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
}

impl ShaderLabPipeline {
    /// Compile the pipeline. Prefer [`compile_source`] first for a clean
    /// error: `create_shader_module` reports failures asynchronously.
    pub fn new(
        gpu: &GpuContext,
        cached: &CachedShader,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, GpuError> {
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader Lab Module"),
            source: wgpu::ShaderSource::Wgsl(cached.wgsl.clone().into()),
        });

        let uniform_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader Lab Uniform Buffer"),
            size: cached.buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Shader Lab Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout =
            gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shader Lab Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        // Placeholder 1x1 texture so the bind group is complete before the
        // first real frame is bound via `render`.
        let placeholder = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Shader Lab Placeholder"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let placeholder_view = placeholder.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader Lab Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&placeholder_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: uniform_buffer.as_entire_binding() },
            ],
        });
        // Keep layout alive via the pipeline layout below.
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shader Lab Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shader Lab Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self { pipeline, uniform_buffer, bind_group, sampler })
    }

    /// Upload one frame of uniforms (runtime + resolved user values).
    pub fn write_frame(
        &self,
        gpu: &GpuContext,
        cached: &CachedShader,
        values: &HashMap<String, ShaderParamValue>,
        defaults: &HashMap<String, ShaderParamValue>,
        time: f32,
        resolution: (f32, f32),
        frame: f32,
        duration: f32,
    ) {
        let buf = build_uniform_buffer(cached, values, defaults, time, resolution, frame, duration);
        gpu.queue.write_buffer(&self.uniform_buffer, 0, &buf);
    }

    /// Render the effect over `src_view` into `target`.
    pub fn render(
        &self,
        gpu: &GpuContext,
        src_view: &wgpu::TextureView,
        layout: &wgpu::BindGroupLayout,
        target: &RenderTarget,
    ) {
        // Rebind with the real source texture each frame (cheap, no pipeline change).
        let frame_bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader Lab Frame Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniform_buffer.as_entire_binding() },
            ],
        });
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Shader Lab Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Shader Lab Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &frame_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit(std::iter::once(encoder.finish()));
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grade_preset_compiles() {
        let (cached, params) = compile_source(project::shader::presets::GRADE).expect("grade compiles");
        assert_eq!(params.len(), 3);
        assert!(cached.wgsl.contains("fn effect("));
        assert!(cached.wgsl.contains("u.brightness"));
        assert!(cached.buffer_size % 16 == 0);
    }

    #[test]
    fn test_all_presets_validate() {
        for src in [
            project::shader::presets::GRADE,
            project::shader::presets::VIGNETTE,
            project::shader::presets::SCANLINES,
            project::shader::presets::DUOTONE,
        ] {
            compile_source(src).expect("preset validates");
        }
    }

    #[test]
    fn test_missing_entry_and_ternary_error() {
        assert!(compile_source("uniform float x = 1.0;\n").is_err());
        let ternary = "uniform float x = 1.0;\nvec4 effect(vec2 uv, vec4 color) {\n float a = x > 0.5 ? 1.0 : 0.0;\n return color;\n}\n";
        let err = compile_source(ternary).expect_err("ternary rejected");
        assert!(err.contains("ternary"), "{err}");
    }

    #[test]
    fn test_failed_source_never_caches() {
        let mut cache = ShaderLabCache::new();
        assert!(cache.get_or_compile("not a shader at all {{{").is_err());
        assert!(cache.is_empty());
        cache
            .get_or_compile(project::shader::presets::GRADE)
            .expect("grade caches");
        assert_eq!(cache.len(), 1);
        // Second fetch is a cache hit (same key, no recompile).
        cache
            .get_or_compile(project::shader::presets::GRADE)
            .expect("grade cache hit");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_uniform_layout_offsets() {
        let (cached, _) = compile_source(
            "uniform float a = 0.0;\nuniform vec3 b = vec3(0.0);\nuniform vec2 c = vec2(0.0);\nvec4 effect(vec2 uv, vec4 color) { return color + vec4(a) + vec4(b, 1.0); }\n",
        )
        .expect("compiles");
        let offsets: HashMap<&str, usize> =
            cached.fields.iter().map(|f| (f.name.as_str(), f.offset)).collect();
        assert_eq!(offsets["time"], 0);
        assert_eq!(offsets["resolution"], 8);
        // a at 24 (f32), b vec3 aligns to 32 (size 12, ends 44),
        // c vec2 aligns to 48.
        assert_eq!(offsets["a"], 24);
        assert_eq!(offsets["b"], 32);
        assert_eq!(offsets["c"], 48);
        assert_eq!(cached.buffer_size % 16, 0);
    }

    #[test]
    fn test_pipeline_compiles_on_device_when_available() {
        let Ok(gpu) = GpuContext::new_headless() else {
            println!("no GPU adapter; skipped");
            return;
        };
        let (cached, _) =
            compile_source(project::shader::presets::GRADE).expect("grade compiles");
        let target = RenderTarget::new(&gpu, 32, 32).expect("target");
        let pipeline =
            ShaderLabPipeline::new(&gpu, &cached, target.format()).expect("pipeline compiles");
        let mut values = HashMap::new();
        values.insert("brightness".to_string(), ShaderParamValue::Float(0.2));
        let defaults: HashMap<String, ShaderParamValue> = HashMap::new();
        // Upload path must not panic; buffer sized by layout.
        pipeline.write_frame(&gpu, &cached, &values, &defaults, 1.0, (32.0, 32.0), 30.0, 5.0);
        let _ = pipeline.bind_group();
    }
}
