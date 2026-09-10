//! `calc()` expressions as interned bytecode evaluated against a percentage basis.

/// Handle to a registered calc expression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CalcId(pub u32);

/// One node of a calc expression tree, in postfix order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalcOp {
    /// Push a pixel constant.
    Px(f32),
    /// Push `fraction * basis`.
    Percent(f32),
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    /// Pops max, value, min.
    Clamp,
}

/// Owns registered expressions; evaluation is allocation free.
#[derive(Clone, Debug, Default)]
pub struct CalcTable {
    ops: Vec<CalcOp>,
    ranges: Vec<(u32, u32)>,
}

impl CalcTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a postfix program and returns its handle.
    pub fn register(&mut self, program: &[CalcOp]) -> CalcId {
        let start = self.ops.len() as u32;
        self.ops.extend_from_slice(program);
        self.ranges.push((start, self.ops.len() as u32));
        CalcId(self.ranges.len() as u32 - 1)
    }

    pub fn resolve(&self, id: CalcId, basis: f32) -> f32 {
        let Some(&(start, end)) = self.ranges.get(id.0 as usize) else { return 0.0 };
        let mut stack = [0.0f32; 16];
        let mut sp = 0usize;
        macro_rules! pop {
            () => {{
                sp -= 1;
                stack[sp]
            }};
        }
        for op in &self.ops[start as usize..end as usize] {
            let v = match *op {
                CalcOp::Px(v) => v,
                CalcOp::Percent(f) => f * basis,
                CalcOp::Add => {
                    let b = pop!();
                    pop!() + b
                }
                CalcOp::Sub => {
                    let b = pop!();
                    pop!() - b
                }
                CalcOp::Mul => {
                    let b = pop!();
                    pop!() * b
                }
                CalcOp::Div => {
                    let b = pop!();
                    pop!() / b
                }
                CalcOp::Min => {
                    let b = pop!();
                    pop!().min(b)
                }
                CalcOp::Max => {
                    let b = pop!();
                    pop!().max(b)
                }
                CalcOp::Clamp => {
                    let max = pop!();
                    let v = pop!();
                    let min = pop!();
                    v.min(max).max(min)
                }
            };
            if sp < stack.len() {
                stack[sp] = v;
                sp += 1;
            }
        }
        if sp > 0 { stack[sp - 1] } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_postfix() {
        let mut t = CalcTable::new();
        let id = t.register(&[CalcOp::Percent(0.5), CalcOp::Px(10.0), CalcOp::Sub]);
        assert_eq!(t.resolve(id, 100.0), 40.0);
        let c = t.register(&[CalcOp::Px(0.0), CalcOp::Percent(1.0), CalcOp::Px(50.0), CalcOp::Clamp]);
        assert_eq!(t.resolve(c, 80.0), 50.0);
        assert_eq!(t.resolve(c, 20.0), 20.0);
    }
}
