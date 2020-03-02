use super::*;

#[derive(Clone, Copy)]
pub(super) struct Instruction(pub(super) u8, pub(super) u8, pub(super) u16);

impl Instruction {
    #[inline]
    pub fn copy(idx: u16, dest: u8) -> Instruction {
        Instruction(0, dest, idx)
    }

    #[inline]
    pub fn constant(idx: u16, dest: u8) -> Instruction {
        Instruction(1, dest, idx)
    }

    #[inline]
    pub fn global(idx: u16, dest: u8) -> Instruction {
        Instruction(2, dest, idx)
    }

    #[inline]
    pub fn add_i(a: u8, b: u8) -> Instruction {
        Instruction(3, a, u16::from(b))
    }
    #[inline]
    pub fn add_f(a: u8, b: u8) -> Instruction {
        Instruction(4, a, u16::from(b))
    }

    #[inline]
    pub fn sub_i(a: u8, b: u8) -> Instruction {
        Instruction(5, a, u16::from(b))
    }
    #[inline]
    pub fn sub_f(a: u8, b: u8) -> Instruction {
        Instruction(6, a, u16::from(b))
    }

    #[inline]
    pub fn mul_i(a: u8, b: u8) -> Instruction {
        Instruction(7, a, u16::from(b))
    }
    #[inline]
    pub fn mul_f(a: u8, b: u8) -> Instruction {
        Instruction(8, a, u16::from(b))
    }

    #[inline]
    pub fn div_i(a: u8, b: u8) -> Instruction {
        Instruction(9, a, u16::from(b))
    }
    #[inline]
    pub fn div_f(a: u8, b: u8) -> Instruction {
        Instruction(10, a, u16::from(b))
    }

    #[inline]
    pub fn rem_i(a: u8, b: u8) -> Instruction {
        Instruction(11, a, u16::from(b))
    }
    #[inline]
    pub fn rem_f(a: u8, b: u8) -> Instruction {
        Instruction(12, a, u16::from(b))
    }

    #[inline]
    pub fn equal_i(a: u8, b: u8) -> Instruction {
        Instruction(13, a, u16::from(b))
    }
    #[inline]
    pub fn equal_f(a: u8, b: u8) -> Instruction {
        Instruction(14, a, u16::from(b))
    }

    #[inline]
    pub fn not_equal_i(a: u8, b: u8) -> Instruction {
        Instruction(15, a, u16::from(b))
    }
    #[inline]
    pub fn not_equal_f(a: u8, b: u8) -> Instruction {
        Instruction(16, a, u16::from(b))
    }

    #[inline]
    pub fn less_equal_i(a: u8, b: u8) -> Instruction {
        Instruction(17, a, u16::from(b))
    }
    #[inline]
    pub fn less_equal_f(a: u8, b: u8) -> Instruction {
        Instruction(18, a, u16::from(b))
    }

    #[inline]
    pub fn greater_equal_i(a: u8, b: u8) -> Instruction {
        Instruction(19, a, u16::from(b))
    }
    #[inline]
    pub fn greater_equal_f(a: u8, b: u8) -> Instruction {
        Instruction(20, a, u16::from(b))
    }

    #[inline]
    pub fn less_i(a: u8, b: u8) -> Instruction {
        Instruction(21, a, u16::from(b))
    }
    #[inline]
    pub fn less_f(a: u8, b: u8) -> Instruction {
        Instruction(22, a, u16::from(b))
    }

    #[inline]
    pub fn greater_i(a: u8, b: u8) -> Instruction {
        Instruction(23, a, u16::from(b))
    }
    #[inline]
    pub fn greater_f(a: u8, b: u8) -> Instruction {
        Instruction(24, a, u16::from(b))
    }

    #[inline]
    pub fn and(a: u8, b: u8) -> Instruction {
        Instruction(25, a, u16::from(b))
    }
    #[inline]
    pub fn or(a: u8, b: u8) -> Instruction {
        Instruction(26, a, u16::from(b))
    }
    #[inline]
    pub fn xor(a: u8, b: u8) -> Instruction {
        Instruction(27, a, u16::from(b))
    }
    #[inline]
    pub fn not(a: u8) -> Instruction {
        Instruction(28, a, 0)
    }

    #[inline]
    pub fn i_to_f(a: u8) -> Instruction {
        Instruction(29, a, 0)
    }
    #[inline]
    pub fn f_to_i(a: u8) -> Instruction {
        Instruction(30, a, 0)
    }
}
impl Debug for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Instruction(0, dest, idx) => write!(f, "copy {}, ={}", idx, dest),
            Instruction(1, dest, idx) => write!(f, "constant {}, ={}", idx, dest),
            Instruction(2, dest, idx) => write!(f, "global {}, ={}", idx, dest),

            Instruction(3, a, b) => write!(f, "add_i ={}, {}", a, b),
            Instruction(4, a, b) => write!(f, "add_f ={}, {}", a, b),

            Instruction(5, a, b) => write!(f, "sub_i ={}, {}", a, b),
            Instruction(6, a, b) => write!(f, "sub_f ={}, {}", a, b),

            Instruction(7, a, b) => write!(f, "mul_i ={}, {}", a, b),
            Instruction(8, a, b) => write!(f, "mul_f ={}, {}", a, b),

            Instruction(9, a, b) => write!(f, "div_i ={}, {}", a, b),
            Instruction(10, a, b) => write!(f, "div_f ={}, {}", a, b),

            Instruction(11, a, b) => write!(f, "rem_i ={}, {}", a, b),
            Instruction(12, a, b) => write!(f, "rem_f ={}, {}", a, b),

            Instruction(13, a, b) => write!(f, "equal_i ={}, {}", a, b),
            Instruction(14, a, b) => write!(f, "equal_f ={}, {}", a, b),

            Instruction(15, a, b) => write!(f, "not_equal_i ={}, {}", a, b),
            Instruction(16, a, b) => write!(f, "not_equal_f ={}, {}", a, b),

            Instruction(17, a, b) => write!(f, "less_equal_i ={}, {}", a, b),
            Instruction(18, a, b) => write!(f, "less_equal_f ={}, {}", a, b),

            Instruction(19, a, b) => write!(f, "greater_equal_i ={}, {}", a, b),
            Instruction(20, a, b) => write!(f, "greater_equal_f ={}, {}", a, b),

            Instruction(21, a, b) => write!(f, "less_i ={}, {}", a, b),
            Instruction(22, a, b) => write!(f, "less_f ={}, {}", a, b),

            Instruction(23, a, b) => write!(f, "greater_i ={}, {}", a, b),
            Instruction(24, a, b) => write!(f, "greater_f ={}, {}", a, b),

            Instruction(25, a, b) => write!(f, "and ={}, {}", a, b),
            Instruction(26, a, b) => write!(f, "or ={}, {}", a, b),
            Instruction(27, a, b) => write!(f, "xor ={}, {}", a, b),
            Instruction(28, a, _) => write!(f, "not ={}", a),

            Instruction(29, a, _) => write!(f, "i_to_f ={}", a),
            Instruction(30, a, _) => write!(f, "f_to_i ={}", a),

            Instruction(_, _, _) => write!(f, "invalid"),
        }
    }
}
