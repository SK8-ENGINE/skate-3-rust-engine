//! Objective conditions as data: the form setup translates retail's Lua predicates into (research b867), evaluated
//! with Lua 5.1's semantics for the subset they use, on retail's 32-bit float numbers (research b869: counters and
//! sums round as retail's do). Defaults on error: a completion condition true, every other kind false (b869). Names (API functions, `vars`, globals) are resolved to indices
//! at load, so evaluation does not allocate for them. docs/hails-additions/42-challenges.md "Decision: objectives as
//! data, not Lua".
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f32),
    Str(Arc<str>),
    /// A table constructor's array part (tutorial rows pass lists to the API).
    List(Arc<[Value]>),
}

impl Value {
    /// Lua truthiness: only nil and false are false.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Sub,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Lit(Value),
    Local(u16),
    Var(u16),
    Global(u16),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    Arith(ArithOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    /// An objective API function by its resolved index.
    Api { func: u16, args: Vec<Expr> },
    /// A call to a function the API does not have (retail's `DidSpecificTrickgroup` typo): an error when reached.
    Missing,
    /// `t[i]` on a list (1-based, as Lua).
    Index(Box<Expr>, Box<Expr>),
    Table(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `local x = e` and `x = e` on a local.
    SetLocal(u16, Expr),
    SetVar(u16, Expr),
    SetGlobal(u16, Expr),
    /// `InitVars(vars, name, value)`: retail's one-line Lua prelude (XEX 820B87D0, research b871) sets the field only
    /// when it is nil; the value is evaluated first, as a call's arguments are.
    InitVar(u16, Expr),
    Call(Expr),
    If { arms: Vec<(Expr, Vec<Stmt>)>, otherwise: Vec<Stmt> },
    For { local: u16, start: Expr, limit: Expr, step: Expr, body: Vec<Stmt> },
    Return(Expr),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub locals: u16,
    pub body: Vec<Stmt>,
}

/// Why an evaluation stopped without a result; the caller then uses the condition's default (retail: a protected
/// call returning the condition's default byte, research b865).
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Arithmetic or ordering on a value that does not allow it, indexing a non-list, a missing function.
    Type(&'static str),
    Api(String),
    /// A loop past the iteration cap (none of the retail loops comes near it).
    TooLong,
}

/// The objective API the conditions call: the game side answers by function index.
pub trait Api {
    fn call(&mut self, func: u16, args: &[Value]) -> Result<Value, Error>;
}

/// A loop's iteration cap.
pub const MAX_ITERATIONS: usize = 10_000;

struct Frame<'a> {
    locals: Vec<Value>,
    vars: &'a mut [Value],
    globals: &'a mut [Value],
    api: &'a mut dyn Api,
}

/// Runs the program: Ok(the returned value), Ok(Nil) when it falls off the end.
pub fn run(program: &Program, api: &mut dyn Api, vars: &mut [Value], globals: &mut [Value]) -> Result<Value, Error> {
    let mut frame = Frame { locals: vec![Value::Nil; program.locals as usize], vars, globals, api };
    Ok(frame.block(&program.body)?.unwrap_or(Value::Nil))
}

/// A condition's answer: a boolean result as returned; an error, a non-boolean or no result gives `default`.
/// Writes made before an error stay, as in retail.
pub fn condition(program: &Program, api: &mut dyn Api, vars: &mut [Value], globals: &mut [Value], default: bool) -> bool {
    match run(program, api, vars, globals) {
        Ok(Value::Bool(b)) => b,
        _ => default,
    }
}

fn slot<'v>(values: &'v mut [Value], i: u16) -> Result<&'v mut Value, Error> {
    values.get_mut(i as usize).ok_or(Error::Type("slot out of range"))
}

impl Frame<'_> {
    fn block(&mut self, body: &[Stmt]) -> Result<Option<Value>, Error> {
        for s in body {
            if let Some(v) = self.stmt(s)? {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    fn stmt(&mut self, s: &Stmt) -> Result<Option<Value>, Error> {
        match s {
            Stmt::SetLocal(i, e) => *slot(&mut self.locals, *i)? = self.expr(e)?,
            Stmt::SetVar(i, e) => *slot(self.vars, *i)? = self.expr(e)?,
            Stmt::SetGlobal(i, e) => *slot(self.globals, *i)? = self.expr(e)?,
            Stmt::InitVar(i, e) => {
                let value = self.expr(e)?;
                let var = slot(self.vars, *i)?;
                if *var == Value::Nil {
                    *var = value;
                }
            }
            Stmt::Call(e) => {
                self.expr(e)?;
            }
            Stmt::If { arms, otherwise } => {
                for (test, body) in arms {
                    if self.expr(test)?.truthy() {
                        return self.block(body);
                    }
                }
                return self.block(otherwise);
            }
            Stmt::For { local, start, limit, step, body } => {
                let (Value::Num(mut i), Value::Num(limit), Value::Num(step)) = (self.expr(start)?, self.expr(limit)?, self.expr(step)?) else {
                    return Err(Error::Type("'for' needs numbers"));
                };
                let mut n = 0;
                while (step > 0. && i <= limit) || (step <= 0. && i >= limit) {
                    n += 1;
                    if n > MAX_ITERATIONS {
                        return Err(Error::TooLong);
                    }
                    *slot(&mut self.locals, *local)? = Value::Num(i);
                    if let Some(v) = self.block(body)? {
                        return Ok(Some(v));
                    }
                    i += step;
                }
            }
            Stmt::Return(e) => return Ok(Some(self.expr(e)?)),
        }
        Ok(None)
    }

    fn expr(&mut self, e: &Expr) -> Result<Value, Error> {
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Local(i) => slot(&mut self.locals, *i)?.clone(),
            Expr::Var(i) => slot(self.vars, *i)?.clone(),
            Expr::Global(i) => slot(self.globals, *i)?.clone(),
            Expr::Not(a) => Value::Bool(!self.expr(a)?.truthy()),
            Expr::And(a, b) => {
                let left = self.expr(a)?;
                if left.truthy() { self.expr(b)? } else { left }
            }
            Expr::Or(a, b) => {
                let left = self.expr(a)?;
                if left.truthy() { left } else { self.expr(b)? }
            }
            Expr::Cmp(op, a, b) => {
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                Value::Bool(compare(*op, &a, &b)?)
            }
            Expr::Arith(op, a, b) => match (self.expr(a)?, self.expr(b)?) {
                (Value::Num(a), Value::Num(b)) => Value::Num(if *op == ArithOp::Add { a + b } else { a - b }),
                _ => return Err(Error::Type("arithmetic on a non-number")),
            },
            Expr::Neg(a) => match self.expr(a)? {
                Value::Num(a) => Value::Num(-a),
                _ => return Err(Error::Type("arithmetic on a non-number")),
            },
            Expr::Api { func, args } => {
                let values = args.iter().map(|a| self.expr(a)).collect::<Result<Vec<_>, _>>()?;
                self.api.call(*func, &values)?
            }
            Expr::Missing => return Err(Error::Type("call to a missing function")),
            Expr::Index(t, i) => match (self.expr(t)?, self.expr(i)?) {
                (Value::List(list), Value::Num(i)) if i.fract() == 0. && i >= 1. => list.get(i as usize - 1).cloned().unwrap_or(Value::Nil),
                (Value::List(_), _) => Value::Nil,
                _ => return Err(Error::Type("index on a non-table")),
            },
            Expr::Table(items) => Value::List(items.iter().map(|a| self.expr(a)).collect::<Result<Vec<_>, _>>()?.into()),
        })
    }
}

/// Lua 5.1: equality never errors (different types are unequal); ordering needs two numbers or two strings.
fn compare(op: CmpOp, a: &Value, b: &Value) -> Result<bool, Error> {
    let order = match (a, b) {
        (Value::Num(x), Value::Num(y)) => x.partial_cmp(y),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        _ => None,
    };
    Ok(match op {
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
        _ => {
            if !matches!((a, b), (Value::Num(_), Value::Num(_)) | (Value::Str(_), Value::Str(_))) {
                return Err(Error::Type("attempt to compare"));
            }
            match (op, order) {
                (CmpOp::Lt, Some(o)) => o.is_lt(),
                (CmpOp::Le, Some(o)) => o.is_le(),
                (CmpOp::Gt, Some(o)) => o.is_gt(),
                (CmpOp::Ge, Some(o)) => o.is_ge(),
                // NaN orders as false both ways.
                _ => false,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Function 0 counts its calls and returns its first argument; function 1 fails.
    struct Fake {
        calls: usize,
    }

    impl Api for Fake {
        fn call(&mut self, func: u16, args: &[Value]) -> Result<Value, Error> {
            self.calls += 1;
            match func {
                0 => Ok(args.first().cloned().unwrap_or(Value::Nil)),
                _ => Err(Error::Api("failed".into())),
            }
        }
    }

    fn lit(v: Value) -> Box<Expr> {
        Box::new(Expr::Lit(v))
    }
    fn num(n: f32) -> Value {
        Value::Num(n)
    }
    fn api(arg: Value) -> Expr {
        Expr::Api { func: 0, args: vec![Expr::Lit(arg)] }
    }
    fn ret(e: Expr) -> Program {
        Program { locals: 1, body: vec![Stmt::Return(e)] }
    }

    #[test]
    fn and_or_return_operands_and_short_circuit() {
        let mut fake = Fake { calls: 0 };
        let p = ret(Expr::Or(lit(Value::Nil), Box::new(api(num(3.)))));
        assert_eq!(run(&p, &mut fake, &mut [], &mut []), Ok(num(3.)));
        let p = ret(Expr::And(lit(Value::Bool(false)), Box::new(api(num(3.)))));
        assert_eq!(run(&p, &mut fake, &mut [], &mut []), Ok(Value::Bool(false)));
        assert_eq!(fake.calls, 1, "the right side of a false 'and' is not called");
        assert!(Value::Num(0.).truthy(), "0 is true in Lua");
    }

    #[test]
    fn errors_and_non_booleans_give_the_default_and_earlier_writes_stay() {
        let mut fake = Fake { calls: 0 };
        let mut vars = [Value::Nil];
        let p = Program { locals: 0, body: vec![
            Stmt::SetVar(0, Expr::Lit(num(1.))),
            Stmt::Return(Expr::Api { func: 1, args: vec![] }),
        ] };
        assert!(condition(&p, &mut fake, &mut vars, &mut [], true));
        assert!(!condition(&p, &mut fake, &mut vars, &mut [], false));
        assert_eq!(vars[0], num(1.));
        assert!(condition(&ret(Expr::Lit(num(1.))), &mut fake, &mut [], &mut [], true), "a number is not a boolean");
        assert!(!condition(&Program::default(), &mut fake, &mut [], &mut [], false), "no result");
        assert!(condition(&ret(Expr::Missing), &mut fake, &mut [], &mut [], true));
    }

    #[test]
    fn comparisons_follow_lua() {
        let mut fake = Fake { calls: 0 };
        let cmp = |op, a, b| ret(Expr::Cmp(op, lit(a), lit(b)));
        assert_eq!(run(&cmp(CmpOp::Eq, Value::Nil, Value::Bool(false)), &mut fake, &mut [], &mut []), Ok(Value::Bool(false)));
        assert_eq!(run(&cmp(CmpOp::Ge, num(5.), num(5.)), &mut fake, &mut [], &mut []), Ok(Value::Bool(true)));
        assert!(run(&cmp(CmpOp::Lt, Value::Nil, num(1.)), &mut fake, &mut [], &mut []).is_err(), "ordering nil errors");
    }

    #[test]
    fn vars_keep_state_between_evaluations() {
        let mut fake = Fake { calls: 0 };
        // InitVar count = 0; count = count + 1; return count >= 2
        let p = Program { locals: 0, body: vec![
            Stmt::InitVar(0, Expr::Lit(num(0.))),
            Stmt::SetVar(0, Expr::Arith(ArithOp::Add, Box::new(Expr::Var(0)), lit(num(1.)))),
            Stmt::Return(Expr::Cmp(CmpOp::Ge, Box::new(Expr::Var(0)), lit(num(2.)))),
        ] };
        let mut vars = [Value::Nil];
        assert!(!condition(&p, &mut fake, &mut vars, &mut [], false));
        assert!(condition(&p, &mut fake, &mut vars, &mut [], false));
    }

    #[test]
    fn if_chains_and_numeric_for() {
        let mut fake = Fake { calls: 0 };
        // local s = 0; for i = 1, 3 do s = s + i end; if s == 6 then return true else return false end
        let p = Program { locals: 2, body: vec![
            Stmt::SetLocal(0, Expr::Lit(num(0.))),
            Stmt::For { local: 1, start: Expr::Lit(num(1.)), limit: Expr::Lit(num(3.)), step: Expr::Lit(num(1.)), body: vec![
                Stmt::SetLocal(0, Expr::Arith(ArithOp::Add, Box::new(Expr::Local(0)), Box::new(Expr::Local(1)))),
            ] },
            Stmt::If { arms: vec![(Expr::Cmp(CmpOp::Eq, Box::new(Expr::Local(0)), lit(num(6.))), vec![Stmt::Return(Expr::Lit(Value::Bool(true)))])],
                otherwise: vec![Stmt::Return(Expr::Lit(Value::Bool(false)))] },
        ] };
        assert!(condition(&p, &mut fake, &mut [], &mut [], false));
        let big = ret(Expr::Cmp(CmpOp::Eq, Box::new(Expr::Arith(ArithOp::Add, lit(num(16_777_216.)), lit(num(1.)))), lit(num(16_777_216.))));
        assert!(condition(&big, &mut fake, &mut [], &mut [], false), "retail's numbers are 32-bit floats");
        let list = ret(Expr::Index(Box::new(Expr::Table(vec![Expr::Lit(num(7.))])), lit(num(1.))));
        assert_eq!(run(&list, &mut fake, &mut [], &mut []), Ok(num(7.)));
    }
}
