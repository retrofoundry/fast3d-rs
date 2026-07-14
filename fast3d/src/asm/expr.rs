//! Expression engine for the `update` block: pure `f32` expressions over `time`/`frame`/`PI`.
//! Deterministic; evaluates `gu*` builder arguments per frame. Hard-errors (no silent defaults).

#[derive(Clone, Copy, Debug)]
pub struct EvalCtx {
    pub time: f32,
    pub frame: f32, // = floor(time * 60), promoted to f32 in expressions
}

#[derive(Clone, Debug, PartialEq)]
pub enum Func {
    Sin,
    Cos,
    Abs,
    Sqrt,
    Floor,
    Min,
    Max,
    Mod,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f32),
    Time,
    Frame,
    Pi,
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExprError(pub String);

impl Expr {
    pub fn eval(&self, ctx: &EvalCtx) -> f32 {
        match self {
            Expr::Num(n) => *n,
            Expr::Time => ctx.time,
            Expr::Frame => ctx.frame,
            Expr::Pi => std::f32::consts::PI,
            Expr::Neg(a) => -a.eval(ctx),
            Expr::Add(a, b) => a.eval(ctx) + b.eval(ctx),
            Expr::Sub(a, b) => a.eval(ctx) - b.eval(ctx),
            Expr::Mul(a, b) => a.eval(ctx) * b.eval(ctx),
            Expr::Div(a, b) => a.eval(ctx) / b.eval(ctx),
            Expr::Call(f, args) => {
                let a = |i: usize| args[i].eval(ctx);
                match f {
                    Func::Sin => a(0).sin(), // RADIANS (matches C sinf)
                    Func::Cos => a(0).cos(),
                    Func::Abs => a(0).abs(),
                    Func::Sqrt => a(0).sqrt(),
                    Func::Floor => a(0).floor(),
                    Func::Min => a(0).min(a(1)),
                    Func::Max => a(0).max(a(1)),
                    Func::Mod => a(0) % a(1), // fmodf semantics; may be negative
                }
            }
        }
    }

    /// True if this expression reads `time` or `frame` (drives `is_time_variant`).
    pub fn references_time(&self) -> bool {
        match self {
            Expr::Time | Expr::Frame => true,
            Expr::Num(_) | Expr::Pi => false,
            Expr::Neg(a) => a.references_time(),
            Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
                a.references_time() || b.references_time()
            }
            Expr::Call(_, args) => args.iter().any(|e| e.references_time()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f32),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
}

fn lex(s: &str) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '+' => {
                out.push(Tok::Plus);
                i += 1;
            }
            '-' => {
                out.push(Tok::Minus);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '/' => {
                out.push(Tok::Slash);
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let start = i;
                while i < b.len() && ((b[i] as char).is_ascii_digit() || b[i] as char == '.') {
                    i += 1;
                }
                let t = &s[start..i];
                let n: f32 = t
                    .parse()
                    .map_err(|_| ExprError(format!("bad number: {t}")))?;
                out.push(Tok::Num(n));
            }
            _ if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] as char == '_')
                {
                    i += 1;
                }
                out.push(Tok::Ident(s[start..i].to_string()));
            }
            _ => return Err(ExprError(format!("unexpected character: {c}"))),
        }
    }
    Ok(out)
}

struct P {
    toks: Vec<Tok>,
    pos: usize,
}

impl P {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn expect(&mut self, t: Tok) -> Result<(), ExprError> {
        if self.bump().as_ref() == Some(&t) {
            Ok(())
        } else {
            Err(ExprError(format!("expected {t:?}")))
        }
    }
    fn expr(&mut self) -> Result<Expr, ExprError> {
        let mut lhs = self.term()?;
        while let Some(op) = self.peek().cloned() {
            match op {
                Tok::Plus => {
                    self.pos += 1;
                    lhs = Expr::Add(Box::new(lhs), Box::new(self.term()?));
                }
                Tok::Minus => {
                    self.pos += 1;
                    lhs = Expr::Sub(Box::new(lhs), Box::new(self.term()?));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }
    fn term(&mut self) -> Result<Expr, ExprError> {
        let mut lhs = self.unary()?;
        while let Some(op) = self.peek().cloned() {
            match op {
                Tok::Star => {
                    self.pos += 1;
                    lhs = Expr::Mul(Box::new(lhs), Box::new(self.unary()?));
                }
                Tok::Slash => {
                    self.pos += 1;
                    lhs = Expr::Div(Box::new(lhs), Box::new(self.unary()?));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }
    fn unary(&mut self) -> Result<Expr, ExprError> {
        if self.peek() == Some(&Tok::Minus) {
            self.pos += 1;
            Ok(Expr::Neg(Box::new(self.unary()?)))
        } else {
            self.atom()
        }
    }
    fn atom(&mut self) -> Result<Expr, ExprError> {
        match self.bump() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LParen) => {
                let e = self.expr()?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen)?;
                    let (f, arity) = match name.as_str() {
                        "sin" => (Func::Sin, 1),
                        "cos" => (Func::Cos, 1),
                        "abs" => (Func::Abs, 1),
                        "sqrt" => (Func::Sqrt, 1),
                        "floor" => (Func::Floor, 1),
                        "min" => (Func::Min, 2),
                        "max" => (Func::Max, 2),
                        "mod" => (Func::Mod, 2),
                        _ => return Err(ExprError(format!("unknown function: {name}"))),
                    };
                    if args.len() != arity {
                        return Err(ExprError(format!(
                            "{name} expects {arity} arg(s), got {}",
                            args.len()
                        )));
                    }
                    Ok(Expr::Call(f, args))
                } else {
                    match name.as_str() {
                        "time" => Ok(Expr::Time),
                        "frame" => Ok(Expr::Frame),
                        "PI" => Ok(Expr::Pi),
                        _ => Err(ExprError(format!("unknown identifier: {name}"))),
                    }
                }
            }
            other => Err(ExprError(format!("unexpected token: {other:?}"))),
        }
    }
}

/// Parse a single expression. Errors on unknown identifiers/functions, arity mismatch,
/// unbalanced parens, and trailing tokens — never defaults silently.
pub fn parse_expr(s: &str) -> Result<Expr, ExprError> {
    let toks = lex(s)?;
    if toks.is_empty() {
        return Err(ExprError("empty expression".into()));
    }
    let mut p = P { toks, pos: 0 };
    let e = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(ExprError("trailing tokens after expression".into()));
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str, t: f32) -> f32 {
        parse_expr(s).unwrap().eval(&EvalCtx {
            time: t,
            frame: (t * 60.0).floor(),
        })
    }

    #[test]
    fn precedence_and_unary() {
        assert_eq!(ev("1 + 2 * 3", 0.0), 7.0);
        assert_eq!(ev("(1 + 2) * 3", 0.0), 9.0);
        assert_eq!(ev("-2 + 5", 0.0), 3.0);
        assert_eq!(ev("10 / 2 / 5", 0.0), 1.0);
    }

    #[test]
    fn builtins_and_funcs() {
        assert_eq!(ev("time * 90", 2.0), 180.0);
        assert_eq!(ev("frame", 1.0), 60.0);
        // frame is the floored 60Hz index (single-owner derivation lives in gbi)
        assert_eq!(ev("frame", 1.004), 60.0);
        assert_eq!(ev("frame", 0.999), 59.0);
        assert!((ev("sin(PI / 2)", 0.0) - 1.0).abs() < 1e-6);
        assert_eq!(ev("max(3, 7)", 0.0), 7.0);
        assert_eq!(ev("floor(2.9)", 0.0), 2.0);
        assert!((ev("mod(7, 3)", 0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn errors_do_not_default() {
        assert!(parse_expr("bogus").is_err()); // unknown identifier
        assert!(parse_expr("sin()").is_err()); // arity
        assert!(parse_expr("min(1)").is_err()); // arity
        assert!(parse_expr("(1 + 2").is_err()); // unbalanced
        assert!(parse_expr("1 2").is_err()); // trailing
        assert!(parse_expr("frobnicate(1)").is_err()); // unknown function
    }

    #[test]
    fn references_time_detection() {
        assert!(parse_expr("time * 90").unwrap().references_time());
        assert!(parse_expr("frame").unwrap().references_time());
        assert!(parse_expr("sin(time)").unwrap().references_time());
        assert!(!parse_expr("45").unwrap().references_time());
        assert!(!parse_expr("2 * PI").unwrap().references_time());
    }
}
