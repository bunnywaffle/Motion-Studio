//! CPU interpreter for the Shader Lab GLSL subset (viewport preview).
//!
//! The GPU path compiles user shaders to WGSL for export; the viewport is
//! div/SVG based, so per-pixel GPU output cannot display there. This module
//! evaluates `vec4 effect(vec2 uv, vec4 color)` on the CPU for a single
//! probe sample (layer average color at the given `uv`, usually center).
//! Solids, shapes, and text flow the result through `processed_color`, so
//! Shader Lab grades show live; images blend a delta overlay from a
//! mid-grey probe. Animated `time` effects preview statically (`time`
//! freezes at the supplied value).
//!
//! Supported subset (mirrors the documented authoring subset):
//! `float/int/uint/bool/vec2/vec3/vec4` scalars, constructors (scalar
//! splat, component lists, vec+scalar mixes), swizzle reads (`.rgb`,
//! `.bgr`, `.xy`, `.a`, ...), arithmetic `+ - * /` (scalar/vector,
//! component-wise), unary `-`/`!`, scalar comparisons, `&&`/`||`,
//! `if`/`else`, local `let`-style declarations (`float x = ...;`,
//! optional `const`), assignment (`= += -= *= /=`), `return`, typed
//! helper functions (no recursion), builtins (`mix`, `clamp`,
//! `smoothstep`, `step`, `sin`, `cos`, `tan`, `pow`, `exp`, `log`,
//! `sqrt`, `abs`, `floor`, `ceil`, `fract`, `min`, `max`, `length`,
//! `distance`, `dot`, `normalize`, scalar/vector casts like
//! `float(x)` / `vec3(x)`), `uniform` declarations with literal defaults,
//! `//` comments, `// @param` / `// @meta` lines, `#version` / `precision`
//! (ignored). Anything else (loops, ternary `?:`, `main()`, preprocessor,
//! write masks, arrays, structs) is a clear error, never silent.

use crate::color::Color;
use crate::shader::ShaderParamValue;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// Runtime value in the interpreter.
#[derive(Debug, Clone, PartialEq)]
pub enum IVal {
    F(f32),
    I(i32),
    B(bool),
    V2([f32; 2]),
    V3([f32; 3]),
    V4([f32; 4]),
}

impl IVal {
    fn as_f32(&self) -> Result<f32, String> {
        match self {
            IVal::F(v) => Ok(*v),
            IVal::I(v) => Ok(*v as f32),
            IVal::B(v) => Ok(if *v { 1.0 } else { 0.0 }),
            _ => Err("expected scalar".to_string()),
        }
    }

    fn as_bool(&self) -> Result<bool, String> {
        match self {
            IVal::B(v) => Ok(*v),
            IVal::F(v) => Ok(*v != 0.0),
            IVal::I(v) => Ok(*v != 0),
            _ => Err("expected boolean".to_string()),
        }
    }

    fn components(&self) -> Vec<f32> {
        match self {
            IVal::F(v) => vec![*v],
            IVal::I(v) => vec![*v as f32],
            IVal::B(v) => vec![if *v { 1.0 } else { 0.0 }],
            IVal::V2(a) => a.to_vec(),
            IVal::V3(a) => a.to_vec(),
            IVal::V4(a) => a.to_vec(),
        }
    }

    fn dim(&self) -> usize {
        match self {
            IVal::F(_) | IVal::I(_) | IVal::B(_) => 1,
            IVal::V2(_) => 2,
            IVal::V3(_) => 3,
            IVal::V4(_) => 4,
        }
    }

    fn is_scalar(&self) -> bool {
        self.dim() == 1
    }

    fn splat_vec(n: usize, v: f32) -> Result<IVal, String> {
        match n {
            2 => Ok(IVal::V2([v; 2])),
            3 => Ok(IVal::V3([v; 3])),
            4 => Ok(IVal::V4([v; 4])),
            _ => Err("bad vector width".to_string()),
        }
    }

    fn from_comps(comps: &[f32]) -> Result<IVal, String> {
        match comps.len() {
            1 => Ok(IVal::F(comps[0])),
            2 => Ok(IVal::V2([comps[0], comps[1]])),
            3 => Ok(IVal::V3([comps[0], comps[1], comps[2]])),
            4 => Ok(IVal::V4([comps[0], comps[1], comps[2], comps[3]])),
            _ => Err(format!("cannot build vector of width {}", comps.len())),
        }
    }

    /// GLSL-style swizzle read (`.x .y .z .w .r .g .b .a .s .t .p .q`).
    fn swizzle(&self, mask: &str) -> Result<IVal, String> {
        let comps = self.components();
        if self.is_scalar() {
            // Scalar swizzle replicates (vec3(x).x style is handled by
            // splat constructors; here `float_var.x` replicates).
            let v = comps[0];
            return IVal::from_comps(&vec![v; mask.len()]);
        }
        let lane = |c: char| -> Result<f32, String> {
            let idx = match c {
                'x' | 'r' | 's' => 0,
                'y' | 'g' | 't' => 1,
                'z' | 'b' | 'p' => 2,
                'w' | 'a' | 'q' => 3,
                _ => return Err(format!("bad swizzle lane '{c}'")),
            };
            comps
                .get(idx)
                .copied()
                .ok_or_else(|| format!("swizzle lane '{c}' out of range"))
        };
        if mask.is_empty() || mask.len() > 4 {
            return Err("bad swizzle mask".to_string());
        }
        let mut out = Vec::with_capacity(mask.len());
        for c in mask.chars() {
            out.push(lane(c)?);
        }
        IVal::from_comps(&out)
    }

    fn add(a: &IVal, b: &IVal) -> Result<IVal, String> {
        Self::arith(a, b, |x, y| x + y, "add")
    }
    fn sub(a: &IVal, b: &IVal) -> Result<IVal, String> {
        Self::arith(a, b, |x, y| x - y, "sub")
    }
    fn mul(a: &IVal, b: &IVal) -> Result<IVal, String> {
        Self::arith(a, b, |x, y| x * y, "mul")
    }
    fn div(a: &IVal, b: &IVal) -> Result<IVal, String> {
        if b.components().iter().any(|v| *v == 0.0) {
            return Err("division by zero".to_string());
        }
        Self::arith(a, b, |x, y| x / y, "div")
    }

    fn arith(a: &IVal, b: &IVal, f: impl Fn(f32, f32) -> f32, _op: &str) -> Result<IVal, String> {
        // Bools never participate in arithmetic.
        if matches!(a, IVal::B(_)) || matches!(b, IVal::B(_)) {
            return Err("bool in arithmetic".to_string());
        }
        let (ac, bc) = (a.components(), b.components());
        if ac.len() == bc.len() {
            let out: Vec<f32> = ac.iter().zip(bc.iter()).map(|(x, y)| f(*x, *y)).collect();
            // Preserve int-ness for int,int.
            if matches!((a, b), (IVal::I(_), IVal::I(_))) && out.len() == 1 {
                return Ok(IVal::I(out[0] as i32));
            }
            return IVal::from_comps(&out);
        }
        if ac.len() == 1 {
            let out: Vec<f32> = bc.iter().map(|y| f(ac[0], *y)).collect();
            return IVal::from_comps(&out);
        }
        if bc.len() == 1 {
            let out: Vec<f32> = ac.iter().map(|x| f(*x, bc[0])).collect();
            return IVal::from_comps(&out);
        }
        Err("vector width mismatch".to_string())
    }

    fn neg(&self) -> Result<IVal, String> {
        match self {
            IVal::F(v) => Ok(IVal::F(-v)),
            IVal::I(v) => Ok(IVal::I(-v)),
            IVal::V2(a) => Ok(IVal::V2([-a[0], -a[1]])),
            IVal::V3(a) => Ok(IVal::V3([-a[0], -a[1], -a[2]])),
            IVal::V4(a) => Ok(IVal::V4([-a[0], -a[1], -a[2], -a[3]])),
            IVal::B(_) => Err("cannot negate bool".to_string()),
        }
    }

    fn not(&self) -> Result<IVal, String> {
        Ok(IVal::B(!self.as_bool()?))
    }

    fn cmp(a: &IVal, b: &IVal, op: &str) -> Result<IVal, String> {
        let (x, y) = (a.as_f32()?, b.as_f32()?);
        let r = match op {
            "<" => x < y,
            "<=" => x <= y,
            ">" => x > y,
            ">=" => x >= y,
            "==" => (x - y).abs() < 1e-6,
            "!=" => (x - y).abs() >= 1e-6,
            _ => return Err("bad comparison".to_string()),
        };
        Ok(IVal::B(r))
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Float(f32),
    Int(i32),
    Bool(bool),
    Punct(String),
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            // Preprocessor line: only #version / precision tolerated.
            let mut j = i;
            while j < chars.len() && chars[j] != '\n' {
                j += 1;
            }
            let line: String = chars[i..j].iter().collect();
            let t = line.trim_start_matches('#').trim();
            if !(t.starts_with("version") || t.starts_with("precision")) {
                return Err(format!("preprocessor not supported: {t}"));
            }
            i = j;
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            loop {
                if i + 1 >= chars.len() {
                    return Err("unterminated block comment".to_string());
                }
                if chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let w: String = chars[start..i].iter().collect();
            match w.as_str() {
                "true" => toks.push(Tok::Bool(true)),
                "false" => toks.push(Tok::Bool(false)),
                _ => toks.push(Tok::Ident(w)),
            }
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let start = i;
            let mut is_float = false;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E' || chars[i] == '+' || chars[i] == '-') {
                // Stop a leading +/- confusion: only allow sign right after e/E.
                if (chars[i] == '+' || chars[i] == '-')
                    && !(i > start && (chars[i - 1] == 'e' || chars[i - 1] == 'E'))
                {
                    break;
                }
                if chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E' {
                    is_float = true;
                }
                i += 1;
            }
            // float suffix like 1.0f
            if i < chars.len() && (chars[i] == 'f' || chars[i] == 'F') {
                is_float = true;
                i += 1;
            }
            // uint suffix like 2u
            let mut is_uint = false;
            if i < chars.len() && (chars[i] == 'u' || chars[i] == 'U') {
                is_uint = true;
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let clean = text.trim_end_matches(['f', 'F', 'u', 'U']);
            if is_float && !is_uint {
                match clean.parse::<f32>() {
                    Ok(v) => toks.push(Tok::Float(v)),
                    Err(_) => return Err(format!("bad float literal {text}")),
                }
            } else {
                match clean.parse::<i32>() {
                    Ok(v) => toks.push(Tok::Int(v)),
                    Err(_) => return Err(format!("bad int literal {text}")),
                }
            }
            continue;
        }
        // Multi-char punctuators.
        let two = if i + 1 < chars.len() {
            Some((chars[i], chars[i + 1]))
        } else {
            None
        };
        let two_op = match two {
            Some(('=', '=')) => Some("=="),
            Some(('!', '=')) => Some("!="),
            Some(('<', '=')) => Some("<="),
            Some(('>', '=')) => Some(">="),
            Some(('&', '&')) => Some("&&"),
            Some(('|', '|')) => Some("||"),
            Some(('+', '=')) => Some("+="),
            Some(('-', '=')) => Some("-="),
            Some(('*', '=')) => Some("*="),
            Some(('/', '=')) => Some("/="),
            _ => None,
        };
        if let Some(op) = two_op {
            toks.push(Tok::Punct(op.to_string()));
            i += 2;
            continue;
        }
        if c == '?' {
            return Err("ternary ?: is not supported (use if or mix)".to_string());
        }
        toks.push(Tok::Punct(c.to_string()));
        i += 1;
    }
    Ok(toks)
}

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Lit(IVal),
    Var(String),
    Swizzle(Box<Expr>, String),
    Call(String, Vec<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(String, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Stmt {
    Decl(String, String, Option<Expr>),
    Assign(String, String, Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Return(Expr),
    ExprStmt(Expr),
}

#[derive(Debug, Clone, PartialEq)]
struct FuncDef {
    params: Vec<String>,
    body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedProg {
    uniforms: Vec<(String, IVal)>,
    entry_uv: String,
    entry_color: String,
    entry_body: Vec<Stmt>,
    funcs: HashMap<String, FuncDef>,
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn new(toks: Vec<Tok>) -> Self {
        Self { toks, pos: 0 }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect_punct(&mut self, p: &str) -> Result<(), String> {
        match self.next() {
            Some(Tok::Punct(q)) if q == p => Ok(()),
            other => Err(format!("expected '{p}', found {other:?}")),
        }
    }

    fn peek_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if q == p)
    }

    fn next_ident(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Tok::Ident(s)) => Ok(s),
            other => Err(format!("expected identifier, found {other:?}")),
        }
    }

    /// Parse a function body: after `ret name` and `(`.
    fn parse_func_body(&mut self) -> Result<FuncDef, String> {
        self.expect_punct("(")?;
        let mut params = Vec::new();
        if !self.peek_punct(")") {
            loop {
                let _ty = self.next_ident()?;
                let pname = self.next_ident()?;
                params.push(pname);
                if self.peek_punct(",") {
                    self.next();
                } else {
                    break;
                }
            }
        }
        self.expect_punct(")")?;
        let body = self.parse_block()?;
        Ok(FuncDef { params, body })
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect_punct("{")?;
        let mut out = Vec::new();
        while !self.peek_punct("}") {
            if self.peek().is_none() {
                return Err("unterminated block".to_string());
            }
            out.push(self.parse_stmt()?);
        }
        self.expect_punct("}")?;
        Ok(out)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        if matches!(self.peek(), Some(Tok::Ident(s)) if s == "if") {
            self.next();
            self.expect_punct("(")?;
            let cond = self.parse_expr()?;
            self.expect_punct(")")?;
            let then_b = self.parse_block()?;
            let mut else_b = Vec::new();
            if matches!(self.peek(), Some(Tok::Ident(s)) if s == "else") {
                self.next();
                if matches!(self.peek(), Some(Tok::Ident(s)) if s == "if") {
                    else_b = vec![self.parse_stmt()?];
                } else {
                    else_b = self.parse_block()?;
                }
            }
            return Ok(Stmt::If(cond, then_b, else_b));
        }
        if matches!(self.peek(), Some(Tok::Ident(s)) if s == "return") {
            self.next();
            let e = self.parse_expr()?;
            self.expect_punct(";")?;
            return Ok(Stmt::Return(e));
        }
        if matches!(self.peek(), Some(Tok::Ident(s)) if s == "for" || s == "while" || s == "do") {
            return Err("loops are not supported in Shader Lab preview".to_string());
        }
        // Declaration (`[const] type name [= expr] ;`) or assignment/call.
        let save = self.pos;
        if let Some(Tok::Ident(ty)) = self.peek().cloned() {
            if is_glsl_type(&ty) {
                self.next();
                if matches!(self.peek(), Some(Tok::Ident(_))) {
                    let vname = self.next_ident()?;
                    if self.peek_punct("(") {
                        return Err("unexpected '(' after declaration".to_string());
                    }
                    let init = if self.peek_punct("=") {
                        self.next();
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                    self.expect_punct(";")?;
                    return Ok(Stmt::Decl(ty, vname, init));
                }
                self.pos = save;
            }
        }
        if matches!(self.peek(), Some(Tok::Ident(s)) if s == "const") {
            self.next();
            let ty = self.next_ident()?;
            if !is_glsl_type(&ty) {
                return Err(format!("unknown type '{ty}'"));
            }
            let vname = self.next_ident()?;
            let init = if self.peek_punct("=") {
                self.next();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect_punct(";")?;
            return Ok(Stmt::Decl(ty, vname, init));
        }
        // Assignment (plain variable only; write masks unsupported).
        if let Some(Tok::Ident(vname)) = self.peek().cloned() {
            // Lookahead for assignment op (but not `==`).
            let is_assign = match self.toks.get(self.pos + 1) {
                Some(Tok::Punct(op)) => ["=", "+=", "-=", "*=", "/="].contains(&op.as_str()),
                _ => false,
            };
            if is_assign {
                self.next();
                let op = match self.next() {
                    Some(Tok::Punct(o)) => o,
                    _ => return Err("bad assignment".to_string()),
                };
                let rhs = self.parse_expr()?;
                self.expect_punct(";")?;
                return Ok(Stmt::Assign(vname, op, rhs));
            }
        }
        // Expression statement (call).
        let e = self.parse_expr()?;
        self.expect_punct(";")?;
        Ok(Stmt::ExprStmt(e))
    }

    fn const_eval(e: &Expr) -> Result<IVal, String> {
        match e {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Neg(x) => Self::const_eval(x)?.neg(),
            Expr::Not(x) => Self::const_eval(x)?.not(),
            Expr::Bin(op, a, b) => {
                let (va, vb) = (Self::const_eval(a)?, Self::const_eval(b)?);
                match op.as_str() {
                    "+" => IVal::add(&va, &vb),
                    "-" => IVal::sub(&va, &vb),
                    "*" => IVal::mul(&va, &vb),
                    "/" => IVal::div(&va, &vb),
                    "<" | "<=" | ">" | ">=" | "==" | "!=" => IVal::cmp(&va, &vb, op),
                    _ => Err("non-constant initializer".to_string()),
                }
            }
            Expr::Call(name, args) => {
                let mut vals = Vec::new();
                for a in args {
                    vals.push(Self::const_eval(a)?);
                }
                eval_constructor(name, &vals)
            }
            Expr::Swizzle(x, m) => Self::const_eval(x)?.swizzle(m),
            _ => Err("non-constant initializer".to_string()),
        }
    }

    // -- expressions (precedence climbing) --

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_and()?;
        while self.peek_punct("||") {
            self.next();
            let r = self.parse_and()?;
            e = Expr::Bin("||".to_string(), Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_eq()?;
        while self.peek_punct("&&") {
            self.next();
            let r = self.parse_eq()?;
            e = Expr::Bin("&&".to_string(), Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_eq(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_rel()?;
        loop {
            let op = if self.peek_punct("==") {
                "=="
            } else if self.peek_punct("!=") {
                "!="
            } else {
                break;
            };
            self.next();
            let r = self.parse_rel()?;
            e = Expr::Bin(op.to_string(), Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_rel(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_add()?;
        loop {
            let op = if self.peek_punct("<=") {
                "<="
            } else if self.peek_punct(">=") {
                ">="
            } else if self.peek_punct("<") {
                "<"
            } else if self.peek_punct(">") {
                ">"
            } else {
                break;
            };
            self.next();
            let r = self.parse_add()?;
            e = Expr::Bin(op.to_string(), Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_mul()?;
        loop {
            let op = if self.peek_punct("+") {
                "+"
            } else if self.peek_punct("-") {
                "-"
            } else {
                break;
            };
            // Disambiguate from `+=`/`-=` (already lexed as one punct).
            self.next();
            let r = self.parse_mul()?;
            e = Expr::Bin(op.to_string(), Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_unary()?;
        loop {
            let op = if self.peek_punct("*") {
                "*"
            } else if self.peek_punct("/") {
                "/"
            } else {
                break;
            };
            self.next();
            let r = self.parse_unary()?;
            e = Expr::Bin(op.to_string(), Box::new(e), Box::new(r));
        }
        Ok(e)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if self.peek_punct("-") {
            self.next();
            return Ok(Expr::Neg(Box::new(self.parse_unary()?)));
        }
        if self.peek_punct("!") {
            self.next();
            return Ok(Expr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_primary()?;
        while self.peek_punct(".") {
            self.next();
            let mask = self.next_ident()?;
            e = Expr::Swizzle(Box::new(e), mask);
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Float(v)) => Ok(Expr::Lit(IVal::F(v))),
            Some(Tok::Int(v)) => Ok(Expr::Lit(IVal::I(v))),
            Some(Tok::Bool(v)) => Ok(Expr::Lit(IVal::B(v))),
            Some(Tok::Ident(name)) => {
                if self.peek_punct("(") {
                    self.next();
                    let mut args = Vec::new();
                    if !self.peek_punct(")") {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.peek_punct(",") {
                                self.next();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_punct(")")?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Some(Tok::Punct(p)) if p == "(" => {
                let e = self.parse_expr()?;
                self.expect_punct(")")?;
                Ok(e)
            }
            other => Err(format!("unexpected token {other:?}")),
        }
    }
}

fn is_glsl_type(t: &str) -> bool {
    matches!(
        t,
        "float" | "int" | "uint" | "bool" | "vec2" | "vec3" | "vec4" | "void"
    )
}

fn default_for_type(ty: &str) -> Result<IVal, String> {
    match ty {
        "float" => Ok(IVal::F(0.0)),
        "int" | "uint" => Ok(IVal::I(0)),
        "bool" => Ok(IVal::B(false)),
        "vec2" => Ok(IVal::V2([0.0; 2])),
        "vec3" => Ok(IVal::V3([0.0; 3])),
        "vec4" => Ok(IVal::V4([0.0; 4])),
        _ => Err(format!("bad type '{ty}'")),
    }
}

/// Constructor / cast evaluation shared by const-eval and runtime.
fn eval_constructor(name: &str, args: &[IVal]) -> Result<IVal, String> {
    match name {
        "float" => {
            if args.len() != 1 {
                return Err("float() takes 1 arg".to_string());
            }
            Ok(IVal::F(args[0].as_f32()?))
        }
        "int" => {
            if args.len() != 1 {
                return Err("int() takes 1 arg".to_string());
            }
            Ok(IVal::I(args[0].as_f32()? as i32))
        }
        "uint" => {
            if args.len() != 1 {
                return Err("uint() takes 1 arg".to_string());
            }
            let v = args[0].as_f32()? as i32;
            Ok(IVal::I(v.max(0)))
        }
        "bool" => {
            if args.len() != 1 {
                return Err("bool() takes 1 arg".to_string());
            }
            Ok(IVal::B(args[0].as_bool()?))
        }
        "vec2" | "vec3" | "vec4" => {
            let want = match name {
                "vec2" => 2,
                "vec3" => 3,
                _ => 4,
            };
            if args.len() == 1 && args[0].is_scalar() {
                return IVal::splat_vec(want, args[0].as_f32()?);
            }
            let mut comps = Vec::new();
            for a in args {
                comps.extend(a.components());
            }
            if comps.len() != want {
                return Err(format!("{name}() needs {want} components, got {}", comps.len()));
            }
            IVal::from_comps(&comps)
        }
        _ => Err(format!("unknown function '{name}'")),
    }
}

// ---------------------------------------------------------------------------
// Top-level parse
// ---------------------------------------------------------------------------

/// Parse a Shader Lab source into an interpretable program.
/// Global initializers become constants; the `effect` entry is required.
pub fn parse_program(source: &str) -> Result<ParsedProg, String> {
    let toks = lex(source)?;
    let mut p = Parser::new(toks);
    // Custom top-level loop: handles the `effect` entry specially.
    let mut uniforms: Vec<(String, IVal)> = Vec::new();
    let mut funcs: HashMap<String, FuncDef> = HashMap::new();
    let mut entry: Option<(String, String, Vec<Stmt>)> = None;
    while p.peek().is_some() {
        let mut is_uniform = false;
        if matches!(p.peek(), Some(Tok::Ident(s)) if s == "uniform") {
            p.next();
            is_uniform = true;
        }
        if matches!(p.peek(), Some(Tok::Ident(s)) if s == "const") {
            p.next();
        }
        if matches!(p.peek(), Some(Tok::Ident(s)) if s == "precision") {
            while !p.peek_punct(";") {
                if p.next().is_none() {
                    return Err("unterminated precision".to_string());
                }
            }
            p.expect_punct(";")?;
            continue;
        }
        let ty = p.next_ident().map_err(|_| "expected declaration".to_string())?;
        if !is_glsl_type(&ty) {
            return Err(format!("unknown type '{ty}' (arrays/structs unsupported)"));
        }
        let name = p.next_ident()?;
        if p.peek_punct("(") {
            if is_uniform {
                return Err("uniform cannot be a function".to_string());
            }
            let func = p.parse_func_body()?;
            if ty == "vec4" && name == "effect" && func.params.len() == 2 {
                if entry.is_some() {
                    return Err("duplicate effect() entry".to_string());
                }
                entry = Some((func.params[0].clone(), func.params[1].clone(), func.body));
            } else if name == "effect" {
                return Err("effect() must be `vec4 effect(vec2, vec4)`".to_string());
            } else {
                if name == "main" {
                    return Err("void main() is not supported (write effect() instead)".to_string());
                }
                if funcs.insert(name.clone(), func).is_some() {
                    return Err(format!("duplicate function '{name}'"));
                }
            }
            continue;
        }
        let init = if p.peek_punct("=") {
            p.next();
            Some(p.parse_expr()?)
        } else {
            None
        };
        p.expect_punct(";")?;
        let val = match init {
            Some(e) => Parser::const_eval(&e)?,
            None => default_for_type(&ty)?,
        };
        if is_uniform {
            uniforms.push((name, val));
        } else {
            uniforms.push((format!(" const {name}"), val));
        }
    }
    match entry {
        Some((uv, color, body)) => Ok(ParsedProg {
            uniforms,
            entry_uv: uv,
            entry_color: color,
            entry_body: body,
            funcs,
        }),
        None => Err("missing `vec4 effect(vec2 uv, vec4 color)` entry".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

struct Interp<'a> {
    prog: &'a ParsedProg,
    scopes: Vec<HashMap<String, IVal>>,
    depth: usize,
}

enum Flow {
    Next,
    Returned(IVal),
}

impl<'a> Interp<'a> {
    fn new(prog: &'a ParsedProg) -> Self {
        Self { prog, scopes: vec![HashMap::new()], depth: 0 }
    }

    fn get(&self, name: &str) -> Option<IVal> {
        // Local scopes (seeded uniform overrides, entry params, locals)
        // shadow declared defaults.
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        for s in self.prog.uniforms.iter() {
            if s.0 == name {
                return Some(s.1.clone());
            }
        }
        None
    }

    fn set(&mut self, name: &str, val: IVal) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return Ok(());
            }
        }
        Err(format!("assign to undeclared '{name}'"))
    }

    fn decl(&mut self, name: &str, val: IVal) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), val);
        }
    }

    fn eval_expr(&mut self, e: &Expr) -> Result<IVal, String> {
        match e {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Var(n) => self.get(n).ok_or_else(|| format!("unknown variable '{n}'")),
            Expr::Swizzle(x, m) => self.eval_expr(x)?.swizzle(m),
            Expr::Neg(x) => self.eval_expr(x)?.neg(),
            Expr::Not(x) => self.eval_expr(x)?.not(),
            Expr::Bin(op, a, b) => {
                if op == "&&" {
                    let va = self.eval_expr(a)?.as_bool()?;
                    if !va {
                        return Ok(IVal::B(false));
                    }
                    return Ok(IVal::B(self.eval_expr(b)?.as_bool()?));
                }
                if op == "||" {
                    let va = self.eval_expr(a)?.as_bool()?;
                    if va {
                        return Ok(IVal::B(true));
                    }
                    return Ok(IVal::B(self.eval_expr(b)?.as_bool()?));
                }
                let (va, vb) = (self.eval_expr(a)?, self.eval_expr(b)?);
                match op.as_str() {
                    "+" => IVal::add(&va, &vb),
                    "-" => IVal::sub(&va, &vb),
                    "*" => IVal::mul(&va, &vb),
                    "/" => IVal::div(&va, &vb),
                    "<" | "<=" | ">" | ">=" | "==" | "!=" => IVal::cmp(&va, &vb, op),
                    _ => Err(format!("bad operator '{op}'")),
                }
            }
            Expr::Call(name, args) => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval_expr(a)?);
                }
                // Constructors / casts first.
                if ["float", "int", "uint", "bool", "vec2", "vec3", "vec4"].contains(&name.as_str()) {
                    return eval_constructor(name, &vals);
                }
                if let Some(r) = eval_builtin(name, &vals)? {
                    return Ok(r);
                }
                self.call_user(name, &vals)
            }
        }
    }

    fn call_user(&mut self, name: &str, args: &[IVal]) -> Result<IVal, String> {
        if self.depth >= 32 {
            return Err("call depth exceeded (recursion unsupported)".to_string());
        }
        let func = self
            .prog
            .funcs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown function '{name}'"))?;
        if func.params.len() != args.len() {
            return Err(format!(
                "{name}() takes {} args, got {}",
                func.params.len(),
                args.len()
            ));
        }
        self.depth += 1;
        self.scopes.push(HashMap::new());
        for (p, v) in func.params.iter().zip(args.iter()) {
            self.decl(p, v.clone());
        }
        let mut ret = IVal::F(0.0);
        let mut broken = false;
        for s in &func.body {
            match self.exec_stmt(s)? {
                Flow::Next => {}
                Flow::Returned(v) => {
                    ret = v;
                    broken = true;
                    break;
                }
            }
        }
        self.scopes.pop();
        self.depth -= 1;
        if !broken {
            return Err(format!("function '{name}' missing return"));
        }
        Ok(ret)
    }

    fn exec_block(&mut self, body: &[Stmt]) -> Result<Flow, String> {
        self.scopes.push(HashMap::new());
        let mut flow = Flow::Next;
        for s in body {
            match self.exec_stmt(s)? {
                Flow::Next => {}
                Flow::Returned(v) => {
                    flow = Flow::Returned(v);
                    break;
                }
            }
        }
        self.scopes.pop();
        Ok(flow)
    }

    fn exec_stmt(&mut self, s: &Stmt) -> Result<Flow, String> {
        match s {
            Stmt::Decl(ty, name, init) => {
                let v = match init {
                    Some(e) => self.eval_expr(e)?,
                    None => default_for_type(ty).unwrap_or(IVal::F(0.0)),
                };
                self.decl(name, v);
                Ok(Flow::Next)
            }
            Stmt::Assign(name, op, rhs) => {
                let cur = self.get(name).ok_or_else(|| format!("assign to undeclared '{name}'"))?;
                let rv = self.eval_expr(rhs)?;
                let next = match op.as_str() {
                    "=" => rv,
                    "+=" => IVal::add(&cur, &rv)?,
                    "-=" => IVal::sub(&cur, &rv)?,
                    "*=" => IVal::mul(&cur, &rv)?,
                    "/=" => IVal::div(&cur, &rv)?,
                    _ => return Err("bad assignment".to_string()),
                };
                self.set(name, next)?;
                Ok(Flow::Next)
            }
            Stmt::If(cond, then_b, else_b) => {
                if self.eval_expr(cond)?.as_bool()? {
                    self.exec_block(then_b)
                } else {
                    self.exec_block(else_b)
                }
            }
            Stmt::Return(e) => Ok(Flow::Returned(self.eval_expr(e)?)),
            Stmt::ExprStmt(e) => {
                self.eval_expr(e)?;
                Ok(Flow::Next)
            }
        }
    }

    fn run_entry(&mut self, uv: (f32, f32), color: [f32; 4]) -> Result<[f32; 4], String> {
        self.scopes.push(HashMap::new());
        self.decl(&self.prog.entry_uv.clone(), IVal::V2([uv.0, uv.1]));
        self.decl(&self.prog.entry_color.clone(), IVal::V4(color));
        let mut out = Err("effect() missing return".to_string());
        // Clone body to satisfy the borrow checker.
        let body = self.prog.entry_body.clone();
        for s in &body {
            match self.exec_stmt(s)? {
                Flow::Next => {}
                Flow::Returned(v) => {
                    out = match v {
                        IVal::V4(a) => Ok(a),
                        IVal::V3(a) => Ok([a[0], a[1], a[2], color[3]]),
                        IVal::F(x) => Ok([x, x, x, color[3]]),
                        other => Err(format!("effect() must return vec4, got {other:?}")),
                    };
                    break;
                }
            }
        }
        self.scopes.pop();
        out
    }
}

fn eval_builtin(name: &str, args: &[IVal]) -> Result<Option<IVal>, String> {
    let one = |i: usize| -> Result<f32, String> {
        args.get(i).ok_or_else(|| format!("{name}() missing arg"))?.as_f32()
    };
    let apply1 = |f: fn(f32) -> f32| -> Result<Option<IVal>, String> {
        if args.len() != 1 {
            return Err(format!("{name}() takes 1 arg"));
        }
        let c: Vec<f32> = args[0].components().iter().map(|v| f(*v)).collect();
        Ok(Some(IVal::from_comps(&c)?))
    };
    match name {
        "radians" => apply1(|v| v.to_radians()),
        "degrees" => apply1(|v| v.to_degrees()),
        "sin" => apply1(|v| v.sin()),
        "cos" => apply1(|v| v.cos()),
        "tan" => apply1(|v| v.tan()),
        "asin" => apply1(|v| v.asin()),
        "acos" => apply1(|v| v.acos()),
        "atan" => {
            if args.len() == 1 {
                apply1(|v| v.atan())
            } else if args.len() == 2 {
                Ok(Some(IVal::F(one(0)?.atan2(one(1)?))))
            } else {
                Err("atan() takes 1-2 args".to_string())
            }
        }
        "exp" => apply1(|v| v.exp()),
        "log" => apply1(|v| v.ln()),
        "exp2" => apply1(|v| v.exp2()),
        "log2" => apply1(|v| v.log2()),
        "sqrt" => apply1(|v| v.sqrt().max(0.0)),
        "inversesqrt" => apply1(|v| 1.0 / v.sqrt().max(1e-6)),
        "abs" => apply1(|v| v.abs()),
        "sign" => apply1(|v| if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 }),
        "floor" => apply1(|v| v.floor()),
        "ceil" => apply1(|v| v.ceil()),
        "fract" => apply1(|v| v - v.floor()),
        "trunc" => apply1(|v| v.trunc()),
        "round" => apply1(|v| v.round()),
        "length" => {
            if args.len() != 1 {
                return Err("length() takes 1 arg".to_string());
            }
            let s: f32 = args[0].components().iter().map(|v| v * v).sum();
            Ok(Some(IVal::F(s.sqrt())))
        }
        "distance" => {
            if args.len() != 2 {
                return Err("distance() takes 2 args".to_string());
            }
            let (a, b) = (args[0].components(), args[1].components());
            if a.len() != b.len() {
                return Err("distance() width mismatch".to_string());
            }
            let s: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum();
            Ok(Some(IVal::F(s.sqrt())))
        }
        "dot" => {
            if args.len() != 2 {
                return Err("dot() takes 2 args".to_string());
            }
            let (a, b) = (args[0].components(), args[1].components());
            if a.len() != b.len() {
                return Err("dot() width mismatch".to_string());
            }
            Ok(Some(IVal::F(a.iter().zip(b.iter()).map(|(x, y)| x * y).sum())))
        }
        "normalize" => {
            if args.len() != 1 {
                return Err("normalize() takes 1 arg".to_string());
            }
            let c = args[0].components();
            let len: f32 = c.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-6);
            Ok(Some(IVal::from_comps(&c.iter().map(|v| v / len).collect::<Vec<_>>())?))
        }
        "min" | "max" => {
            if args.len() != 2 {
                return Err(format!("{name}() takes 2 args"));
            }
            let (a, b) = (&args[0], &args[1]);
            if a.dim() == 1 && b.dim() == 1 {
                let (x, y) = (a.as_f32()?, b.as_f32()?);
                return Ok(Some(IVal::F(if name == "min" { x.min(y) } else { x.max(y) })));
            }
            if a.dim() == b.dim() {
                let (ac, bc) = (a.components(), b.components());
                let out: Vec<f32> = ac
                    .iter()
                    .zip(bc.iter())
                    .map(|(x, y)| if name == "min" { x.min(*y) } else { x.max(*y) })
                    .collect();
                return Ok(Some(IVal::from_comps(&out)?));
            }
            // Scalar-vector form.
            let (vec, s) = if !a.is_scalar() { (a, b.as_f32()?) } else { (b, a.as_f32()?) };
            let out: Vec<f32> = vec
                .components()
                .iter()
                .map(|x| if name == "min" { x.min(s) } else { x.max(s) })
                .collect();
            return Ok(Some(IVal::from_comps(&out)?));
        }
        "clamp" => {
            if args.len() != 3 {
                return Err("clamp() takes 3 args".to_string());
            }
            let (x, lo, hi) = (&args[0], &args[1], &args[2]);
            let lc = lo.components();
            let hc = hi.components();
            if lc.len() != 1 || hc.len() != 1 {
                // Vector bounds must match width (covers clamp(c, vec3(0), vec3(1))).
                let xc = x.components();
                if lc.len() != xc.len() || hc.len() != xc.len() {
                    return Err("clamp() width mismatch".to_string());
                }
                let out: Vec<f32> = xc
                    .iter()
                    .zip(lc.iter())
                    .zip(hc.iter())
                    .map(|((v, l), h)| v.clamp(*l, *h))
                    .collect();
                return Ok(Some(IVal::from_comps(&out)?));
            }
            let (l, h) = (lc[0], hc[0]);
            let out: Vec<f32> = x.components().iter().map(|v| v.clamp(l, h)).collect();
            return Ok(Some(IVal::from_comps(&out)?));
        }
        "mix" => {
            if args.len() != 3 {
                return Err("mix() takes 3 args".to_string());
            }
            let (x, y, t) = (&args[0], &args[1], &args[2]);
            let (xc, yc) = (x.components(), y.components());
            if xc.len() != yc.len() {
                return Err("mix() width mismatch".to_string());
            }
            let tc = t.components();
            let out: Vec<f32> = xc
                .iter()
                .zip(yc.iter())
                .enumerate()
                .map(|(i, (a, b))| {
                    let k = if tc.len() == 1 { tc[0] } else { tc.get(i).copied().unwrap_or(tc[0]) };
                    a + (b - a) * k.clamp(0.0, 1.0)
                })
                .collect();
            return Ok(Some(IVal::from_comps(&out)?));
        }
        "step" => {
            if args.len() != 2 {
                return Err("step() takes 2 args".to_string());
            }
            let (edge, x) = (one(0)?, one(1)?);
            return Ok(Some(IVal::F(if x < edge { 0.0 } else { 1.0 })));
        }
        "smoothstep" => {
            if args.len() != 3 {
                return Err("smoothstep() takes 3 args".to_string());
            }
            let (e0, e1, x) = (one(0)?, one(1)?, one(2)?);
            let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
            return Ok(Some(IVal::F(t * t * (3.0 - 2.0 * t))));
        }
        "pow" => {
            if args.len() != 2 {
                return Err("pow() takes 2 args".to_string());
            }
            return Ok(Some(IVal::F(one(0)?.max(0.0).powf(one(1)?))));
        }
        "mod" => {
            if args.len() != 2 {
                return Err("mod() takes 2 args".to_string());
            }
            let (x, y) = (one(0)?, one(1)?);
            if y == 0.0 {
                return Err("mod() by zero".to_string());
            }
            return Ok(Some(IVal::F(x - y * (x / y).floor())));
        }
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

/// Uniform overrides for preview evaluation.
#[derive(Debug, Clone, Default)]
pub struct PreviewEnv {
    /// User uniform values (falls back to declared defaults).
    pub values: HashMap<String, ShaderParamValue>,
    /// Reserved `time` uniform.
    pub time: f32,
    /// Reserved `frame` uniform.
    pub frame: f32,
    /// Reserved `duration` uniform.
    pub duration: f32,
    /// Reserved `resolution` uniform (defaults to 1x1 when unknown).
    pub resolution: (f32, f32),
}

/// Evaluate `effect(uv, color)` for one probe sample.
/// Returns the output RGBA; errors describe the first unsupported construct.
pub fn eval_effect(
    source: &str,
    env: &PreviewEnv,
    uv: (f32, f32),
    color: Color,
) -> Result<[f32; 4], String> {
    let prog = parse_program(source)?;
    eval_prog(&prog, env, uv, color)
}

/// Evaluate an already-parsed program (hot path for layer previews:
/// parse once at evaluation, interpret per sample).
pub fn eval_prog(
    prog: &ParsedProg,
    env: &PreviewEnv,
    uv: (f32, f32),
    color: Color,
) -> Result<[f32; 4], String> {
    // Seed scope: declared uniform defaults, overridden by user values,
    // then reserved uniforms.
    let mut interp = Interp::new(prog);
    for (name, def) in &prog.uniforms {
        if name.starts_with(" const ") {
            continue;
        }
        let v = match env.values.get(name) {
            Some(ov) => param_value_to_ival(ov),
            None => def.clone(),
        };
        interp.decl(name, v);
    }
    for (name, val) in &prog.uniforms {
        if let Some(real) = name.strip_prefix(" const ") {
            interp.decl(real, val.clone());
        }
    }
    interp.decl("time", IVal::F(env.time));
    interp.decl("frame", IVal::F(env.frame));
    interp.decl("duration", IVal::F(env.duration));
    interp.decl(
        "resolution",
        IVal::V2([env.resolution.0, env.resolution.1]),
    );
    let out = interp.run_entry(uv, [color.r, color.g, color.b, color.a])?;
    Ok(out)
}

fn param_value_to_ival(v: &ShaderParamValue) -> IVal {
    match v {
        ShaderParamValue::Float(x) => IVal::F(*x),
        ShaderParamValue::Int(x) => IVal::I(*x),
        ShaderParamValue::Bool(x) => IVal::B(*x),
        ShaderParamValue::Vec2(a) => IVal::V2(*a),
        ShaderParamValue::Vec3(a) => IVal::V3(*a),
        ShaderParamValue::Vec4(a) => IVal::V4(*a),
        ShaderParamValue::Color(c) => IVal::V4([c.r, c.g, c.b, c.a]),
    }
}

/// Convenience: probe a shader on a color with default env (`time` = 0).
/// Returns `None` when the source uses unsupported constructs.
pub fn preview_color(source: &str, values: &HashMap<String, ShaderParamValue>, color: Color) -> Option<Color> {
    let env = PreviewEnv { values: values.clone(), ..Default::default() };
    match eval_effect(source, &env, (0.5, 0.5), color) {
        Ok([r, g, b, a]) => Some(Color::rgba(r, g, b, a)),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::presets;

    fn env() -> PreviewEnv {
        PreviewEnv::default()
    }

    #[test]
    fn grade_identity_by_default() {
        let out = eval_effect(presets::GRADE, &env(), (0.5, 0.5), Color::rgba(0.2, 0.4, 0.6, 1.0)).unwrap();
        for (a, b) in [out[0], out[1], out[2]].iter().zip([0.2, 0.4, 0.6]) {
            assert!((a - b).abs() < 1e-4, "{out:?}");
        }
    }

    #[test]
    fn grade_brightness_lifts() {
        let mut values = HashMap::new();
        values.insert("brightness".to_string(), ShaderParamValue::Float(0.5));
        let e = PreviewEnv { values, ..Default::default() };
        let out = eval_effect(presets::GRADE, &e, (0.5, 0.5), Color::rgba(0.2, 0.2, 0.2, 1.0)).unwrap();
        assert!((out[0] - 0.7).abs() < 1e-4, "{out:?}");
    }

    #[test]
    fn vignette_darkens_corners_not_center() {
        let c = Color::rgba(0.8, 0.8, 0.8, 1.0);
        let center = eval_effect(presets::VIGNETTE, &env(), (0.5, 0.5), c).unwrap();
        assert!((center[0] - 0.8).abs() < 0.05, "{center:?}");
        let corner = eval_effect(presets::VIGNETTE, &env(), (0.0, 0.0), c).unwrap();
        assert!(corner[0] < 0.4, "{corner:?}");
    }

    #[test]
    fn scanlines_modulate_vertically() {
        let c = Color::rgba(1.0, 1.0, 1.0, 1.0);
        // Flicker off by default: deterministic. One full sine period spans
        // 1/density in uv.y; sample trough (y=0) vs peak (quarter period).
        let trough = eval_effect(presets::SCANLINES, &env(), (0.5, 0.0), c).unwrap();
        let peak = eval_effect(presets::SCANLINES, &env(), (0.5, 1.0 / (4.0 * 240.0)), c).unwrap();
        assert!((peak[0] - trough[0]).abs() > 0.1, "{peak:?} vs {trough:?}");
    }

    #[test]
    fn duotone_maps_luminance() {
        let black = eval_effect(presets::DUOTONE, &env(), (0.5, 0.5), Color::rgba(0.0, 0.0, 0.0, 1.0)).unwrap();
        assert!(black[0] < 0.15 && black[2] > 0.15, "{black:?}");
        let white = eval_effect(presets::DUOTONE, &env(), (0.5, 0.5), Color::rgba(1.0, 1.0, 1.0, 1.0)).unwrap();
        assert!(white[0] > 0.85, "{white:?}");
    }

    #[test]
    fn rejects_unsupported_constructs() {
        assert!(parse_program("void main() {}").is_err());
        assert!(parse_program("vec4 effect(vec2 uv, vec4 c) { return true ? c : c; }").is_err());
        assert!(parse_program("vec4 effect(vec2 uv, vec4 c) { for (int i = 0; i < 3; i++) {} return c; }").is_err());
        assert!(parse_program("vec4 effect(vec2 uv, vec4 c) { c.rgb = vec3(1.0); return c; }").is_err());
    }

    #[test]
    fn helpers_if_and_swizzle() {
        let src = r#"
uniform float k = 2.0;
float doubleit(float x) { return x * 2.0; }
vec4 effect(vec2 uv, vec4 color) {
    float v = doubleit(k);
    vec3 c = color.bgr;
    if (v > 3.0) {
        c = c + vec3(0.1);
    } else {
        c = c - vec3(0.1);
    }
    return vec4(c, 1.0);
}
"#;
        let out = eval_effect(src, &env(), (0.5, 0.5), Color::rgba(0.2, 0.4, 0.6, 1.0)).unwrap();
        // bgr of (0.2,0.4,0.6) = (0.6,0.4,0.2); k=2 -> doubleit=4 > 3 -> +0.1
        assert!((out[0] - 0.7).abs() < 1e-4, "{out:?}");
        assert!((out[2] - 0.3).abs() < 1e-4, "{out:?}");
    }
}
