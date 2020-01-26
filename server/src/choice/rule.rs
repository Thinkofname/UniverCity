
use super::*;



/// A list of compiled rules that can be used to decide whether
/// a choice could be used.
#[derive(Debug)]
pub struct Rules {
    instructions: Vec<Instruction>,
    constants: Vec<u32>,
    memory_size: usize,
}

impl Rules {
    /// Parses a list of rules into a single list of instructions
    pub fn parse(valloc: &mut impl VariableAllocator, rules: &[impl AsRef<str>]) -> UResult<Rules> {
        // Parse and merge rules
        let rule_expr = rules.iter()
            .map(|v| parse::parse(v.as_ref()))
            .try_fold(parse::ExprInner::Boolean(true).into(), |cur, next| {
                next.map(|v| parse::ExprInner::And(Box::new(cur), Box::new(v)).into())
            })?;

        let mut rule_expr = optimize_expr(rule_expr);
        infer_types(valloc, &mut rule_expr, Some(Type::Boolean))?;

        let mut lalloc = LocalAlloc {
            stack_slots: Vec::with_capacity(4),
            constants: Vec::new(),
            max_size: 0
        };
        let mut instructions = vec![];

        let result = gen_expr(valloc, &mut lalloc, &mut instructions, rule_expr);

        assert_eq!(result, 0);
        assert!(lalloc.max_size > 0);

        Ok(Rules {
            instructions,
            constants: lalloc.constants,
            memory_size: lalloc.max_size,
        })
    }

    /// Executes the rules using the passed variables
    ///
    /// Returns whether the rules match or not.
    pub fn execute(&self, vars: &impl VariableAccess) -> bool {
        let mut memory = vec![0; self.memory_size];

        unsafe {
            for instr in &self.instructions {
                match *instr {
                    // copy
                    Instruction(0, dest, idx) => {
                        *memory.get_unchecked_mut(dest as usize) = vars.get(idx);
                    },
                    // constant
                    Instruction(1, dest, idx) => {
                        *memory.get_unchecked_mut(dest as usize) = *self.constants.get_unchecked(idx as usize);
                    },
                    // global
                    Instruction(2, dest, idx) => {
                        *memory.get_unchecked_mut(dest as usize) = vars.get_global(idx);
                    },

                    // add_i
                    Instruction(3, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        // Signed/unsigned math is the same for adding
                        *memory.get_unchecked_mut(a as usize) = av.wrapping_add(bv);
                    },
                    // add_f
                    Instruction(4, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) + f32::from_bits(bv)).to_bits();
                    },

                    // sub_i
                    Instruction(5, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        // Signed/unsigned math is the same for subtracting
                        *memory.get_unchecked_mut(a as usize) = av.wrapping_sub(bv);
                    },
                    // sub_f
                    Instruction(6, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) - f32::from_bits(bv)).to_bits();
                    },

                    // mul_i
                    Instruction(7, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (av as i32).wrapping_mul(bv as i32) as u32;
                    },
                    // mul_f
                    Instruction(8, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) * f32::from_bits(bv)).to_bits();
                    },

                    // div_i
                    Instruction(9, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (av as i32 / bv as i32) as u32;
                    },
                    // div_f
                    Instruction(10, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) / f32::from_bits(bv)).to_bits();
                    },

                    // rem_i
                    Instruction(11, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (av as i32 % bv as i32) as u32;
                    },
                    // rem_f
                    Instruction(12, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) % f32::from_bits(bv)).to_bits();
                    },

                    // equal_i
                    Instruction(13, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = ((av as i32) == (bv as i32)) as u32;
                    },
                    // equal_f
                    Instruction(14, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) == f32::from_bits(bv)) as u32;
                    },

                    // not_equal_i
                    Instruction(15, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = ((av as i32) != (bv as i32)) as u32;
                    },
                    // not_equal_f
                    Instruction(16, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) != f32::from_bits(bv)) as u32;
                    },

                    // less_equal_i
                    Instruction(17, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = ((av as i32) <= (bv as i32)) as u32;
                    },
                    // less_equal_f
                    Instruction(18, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) <= f32::from_bits(bv)) as u32;
                    },

                    // greater_equal_i
                    Instruction(19, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = ((av as i32) >= (bv as i32)) as u32;
                    },
                    // greater_equal_f
                    Instruction(20, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) >= f32::from_bits(bv)) as u32;
                    },

                    // less_i
                    Instruction(21, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = ((av as i32) < (bv as i32)) as u32;
                    },
                    // less_f
                    Instruction(22, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) < f32::from_bits(bv)) as u32;
                    },

                    // greater_i
                    Instruction(23, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = ((av as i32) > (bv as i32)) as u32;
                    },
                    // greater_f
                    Instruction(24, a, b) => {
                        let av = *memory.get_unchecked(a as usize);
                        let bv = *memory.get_unchecked(b as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) > f32::from_bits(bv)) as u32;
                    },

                    // and
                    Instruction(25, a, b) => {
                        let av = *memory.get_unchecked(a as usize) != 0;
                        let bv = *memory.get_unchecked(b as usize) != 0;
                        *memory.get_unchecked_mut(a as usize) = (av && bv) as u32;
                    },
                    // or
                    Instruction(26, a, b) => {
                        let av = *memory.get_unchecked(a as usize) != 0;
                        let bv = *memory.get_unchecked(b as usize) != 0;
                        *memory.get_unchecked_mut(a as usize) = (av || bv) as u32;
                    },
                    // xor
                    Instruction(27, a, b) => {
                        let av = *memory.get_unchecked(a as usize) != 0;
                        let bv = *memory.get_unchecked(b as usize) != 0;
                        *memory.get_unchecked_mut(a as usize) = (av ^ bv) as u32;
                    },
                    // not
                    Instruction(28, a, _) => {
                        let av = *memory.get_unchecked(a as usize);
                        *memory.get_unchecked_mut(a as usize) = (av == 0) as u32;
                    },

                    // i_to_f
                    Instruction(29, a, _) => {
                        let av = *memory.get_unchecked(a as usize);
                        *memory.get_unchecked_mut(a as usize) = (av as i32 as f32).to_bits();
                    },
                    // f_to_i
                    Instruction(30, a, _) => {
                        let av = *memory.get_unchecked(a as usize);
                        *memory.get_unchecked_mut(a as usize) = (f32::from_bits(av) as i32) as u32;
                    },

                    Instruction(_, _, _) => ::std::hint::unreachable_unchecked(),
                }
            }
        }

        unsafe {
            *memory.get_unchecked(0) != 0
        }
    }
}

/// Used during compiling to obtain indexes for variables
/// used by rules.
pub trait VariableAllocator {
    /// Returns the storage location for a given variable with the
    /// passed type, should return the expected type if the passed
    /// type doesn't match.
    fn storage_loc(&mut self, ty: Type, name: &str) -> Result<u16, Type>;
    /// Returns the type of the variable if it is known.
    fn storage_ty(&mut self, name: &str) -> Option<Type>;
    /// Returns the storage location for a given variable without
    /// caring for the type.
    fn raw_storage_loc(&mut self, name: &str) -> u16;

    /// Returns the storage location for a given global variable with the
    /// passed type, should return the expected type if the passed
    /// type doesn't match.
    fn global_loc(&mut self, ty: Type, name: &str) -> Result<u16, Type>;
    /// Returns the type of the global variable if it is known.
    fn global_ty(&mut self, name: &str) -> Option<Type>;
    /// Returns the storage location for a given variable without
    /// caring for the type.
    fn raw_global_loc(&mut self, name: &str) -> u16;
}

/// Used during execution to obtain the value of a variable
pub trait VariableAccess {
    /// Returns the variable value for the given index.
    ///
    /// You can safely assume that `execute` will only
    /// use indices that were previously given to `VariableAllocator`.
    unsafe fn get(&self, idx: u16) -> u32;

    /// Returns the global variable value for the given index.
    ///
    /// You can safely assume that `execute` will only
    /// use indices that were previously given to `VariableAllocator`.
    unsafe fn get_global(&self, idx: u16) -> u32;
}

impl VariableAllocator for () {
    #[cold]
    fn storage_loc(&mut self, _ty: Type, _name: &str) -> Result<u16, Type> {
        panic!("No support for variables on allocator")
    }

    #[cold]
    fn raw_storage_loc(&mut self, _name: &str) -> u16 {
        panic!("No support for variables on allocator")
    }

    #[cold]
    fn storage_ty(&mut self, _name: &str) -> Option<Type> {
        panic!("No support for variables on allocator")
    }

    #[cold]
    fn global_loc(&mut self, _ty: Type, _name: &str) -> Result<u16, Type> {
        panic!("No support for globals on allocator")
    }

    #[cold]
    fn raw_global_loc(&mut self, _name: &str) -> u16 {
        panic!("No support for globals on allocator")
    }

    #[cold]
    fn global_ty(&mut self, _name: &str) -> Option<Type> {
        panic!("No support for variables on allocator")
    }
}

/// A simple variable allocator
pub struct BasicAlloc<G> {
    /// The map of names to indices and types
    pub storage_ty: FNVMap<String, (Type, u16)>,
    global: G,
}

impl <G: VariableAllocator> BasicAlloc<G> {
    /// Returns a new allocator
    pub fn new(global: G) -> BasicAlloc<G> {
        BasicAlloc {
            storage_ty: Default::default(),
            global,
        }
    }

    /// Splits off the global allocator for reuse
    pub fn remove_global(self) -> (BasicAlloc<()>, G) {
        (BasicAlloc {
            storage_ty: self.storage_ty,
            global: (),
        }, self.global)
    }
}

impl <G: VariableAllocator> VariableAllocator for BasicAlloc<G> {
    #[inline]
    fn storage_loc(&mut self, ty: Type, name: &str) -> Result<u16, Type> {
        let next = self.storage_ty.len() as u16;
        let used_ty = *self.storage_ty.entry(name.into()).or_insert((ty, next));
        if used_ty.0 != ty {
            Err(used_ty.0)
        } else {
            Ok(used_ty.1)
        }
    }

    #[inline]
    fn storage_ty(&mut self, name: &str) -> Option<Type> {
        if let Some((ty, _)) = self.storage_ty.get(name) {
            Some(*ty)
        } else {
            None
        }
    }

    #[inline]
    fn raw_storage_loc(&mut self, name: &str) -> u16 {
        self.storage_ty[name].1
    }

    #[inline]
    fn global_loc(&mut self, ty: Type, name: &str) -> Result<u16, Type> {
        self.global.storage_loc(ty, name)
    }

    #[inline]
    fn global_ty(&mut self, name: &str) -> Option<Type> {
        self.global.storage_ty(name)
    }

    #[inline]
    fn raw_global_loc(&mut self, name: &str) -> u16 {
        self.global.raw_storage_loc(name)
    }
}

fn gen_expr(
    valloc: &mut impl VariableAllocator,
    lalloc: &mut LocalAlloc,
    instructions: &mut Vec<Instruction>,
    expr: parse::Expr<'_>
) -> usize {
    use self::parse::ExprInner::*;
    match expr.inner {
        Get(name) => {
            let idx = valloc.raw_storage_loc(name);
            let reg = lalloc.get();
            instructions.push(Instruction::copy(idx, reg as u8));
            reg
        }
        GetGlobal(name) => {
            let idx = valloc.raw_global_loc(name);
            let reg = lalloc.get();
            instructions.push(Instruction::global(idx, reg as u8));
            reg
        },

        Boolean(b) => {
            let reg = lalloc.get();
            let constant = lalloc.add_constant(b as u32);
            instructions.push(Instruction::constant(constant as u16, reg as u8));
            reg
        },
        Float(f) => {
            let reg = lalloc.get();
            let constant = lalloc.add_constant(f.to_bits());
            instructions.push(Instruction::constant(constant as u16, reg as u8));
            reg
        },
        Integer(i) => {
            let reg = lalloc.get();
            let constant = lalloc.add_constant(i as u32);
            instructions.push(Instruction::constant(constant as u16, reg as u8));
            reg
        },

        Not(e) => {
            let er = gen_expr(valloc, lalloc, instructions, *e);
            instructions.push(Instruction::not(er as u8));
            er
        },
        And(l, r) => {
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(Instruction::and(lr as u8, rr as u8));
            lalloc.free(rr);
            lr
        },
        Or(l, r) => {
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(Instruction::or(lr as u8, rr as u8));
            lalloc.free(rr);
            lr
        },
        Xor(l, r) => {
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(Instruction::xor(lr as u8, rr as u8));
            lalloc.free(rr);
            lr
        },

        Add(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::add_i(lr as u8, rr as u8),
                Type::Float => Instruction::add_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        Sub(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::sub_i(lr as u8, rr as u8),
                Type::Float => Instruction::sub_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        Mul(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::mul_i(lr as u8, rr as u8),
                Type::Float => Instruction::mul_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        Div(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::div_i(lr as u8, rr as u8),
                Type::Float => Instruction::div_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        Rem(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::rem_i(lr as u8, rr as u8),
                Type::Float => Instruction::rem_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },

        Equal(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::equal_i(lr as u8, rr as u8),
                Type::Float => Instruction::equal_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        NotEqual(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::not_equal_i(lr as u8, rr as u8),
                Type::Float => Instruction::not_equal_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        LessEqual(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::less_equal_i(lr as u8, rr as u8),
                Type::Float => Instruction::less_equal_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        GreaterEqual(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::greater_equal_i(lr as u8, rr as u8),
                Type::Float => Instruction::greater_equal_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        Less(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::less_i(lr as u8, rr as u8),
                Type::Float => Instruction::less_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },
        Greater(l, r) => {
            let ty = l.ty.expect("Missing type");
            let lr = gen_expr(valloc, lalloc, instructions, *l);
            let rr = gen_expr(valloc, lalloc, instructions, *r);
            instructions.push(match ty {
                Type::Boolean => unreachable!(),
                Type::Integer => Instruction::greater_i(lr as u8, rr as u8),
                Type::Float => Instruction::greater_f(lr as u8, rr as u8),
            });
            lalloc.free(rr);
            lr
        },

        IntToFloat(e) => {
            let er = gen_expr(valloc, lalloc, instructions, *e);
            instructions.push(Instruction::i_to_f(er as u8));
            er
        },
        FloatToInt(e) => {
            let er = gen_expr(valloc, lalloc, instructions, *e);
            instructions.push(Instruction::f_to_i(er as u8));
            er
        },
    }
}

struct LocalAlloc {
    stack_slots: Vec<bool>,
    constants: Vec<u32>,
    max_size: usize,
}

impl LocalAlloc {
    fn get(&mut self) -> usize {
        if let Some(free) = self.stack_slots.iter_mut()
            .enumerate()
            .find(|v| !*v.1) {
            *free.1 = true;
            return free.0;
        }
        let id = self.stack_slots.len();
        self.stack_slots.push(true);
        self.max_size = self.stack_slots.len();
        id
    }

    fn free(&mut self, id: usize) {
        self.stack_slots[id] = false;
    }

    fn add_constant(&mut self, val: u32) -> usize {
        let id = self.constants.len();
        self.constants.push(val);
        id
    }
}

#[test]
fn test_rules() {
    let galloc = BasicAlloc::new(());
    let mut alloc = BasicAlloc::new(galloc);
    let rules = Rules::parse(&mut alloc, &[
        "a + b < 2 * 5",
        "a == global.test"
    ]).unwrap();

    struct VMem {
        memory: Vec<u32>,
        global_memory: Vec<u32>,
    }

    let (alloc, galloc) = alloc.remove_global();

    let mut variable_memory = VMem {
        memory: vec![0; alloc.storage_ty.len()],
        global_memory: vec![0; galloc.storage_ty.len()],
    };

    impl VariableAccess for VMem {
        unsafe fn get(&self, idx: u16) -> u32 {
            // Since this is a test we'll do it safely
            self.memory[idx as usize]
        }
        unsafe fn get_global(&self, idx: u16) -> u32 {
            // Since this is a test we'll do it safely
            self.global_memory[idx as usize]
        }
    }
    variable_memory.global_memory[0] = 2;

    variable_memory.memory[alloc.storage_ty["a"].1 as usize] = 2i32 as u32;
    variable_memory.memory[alloc.storage_ty["b"].1 as usize] = 6i32 as u32;

    assert_eq!(rules.execute(&variable_memory), true);

    variable_memory.global_memory[0] = 3;
    variable_memory.memory[alloc.storage_ty["a"].1 as usize] = 8i32 as u32;
    variable_memory.memory[alloc.storage_ty["b"].1 as usize] = 5i32 as u32;

    assert_eq!(rules.execute(&variable_memory), false);
}

fn infer_types(valloc: &mut impl VariableAllocator, expr: &mut parse::Expr<'_>, expected: Option<Type>) -> UResult<()> {
    use self::parse::ExprInner::*;
    match &mut expr.inner {
        Get(name) => {
            if let Some(exp) = expected {
                if let Err(var_ty) =  valloc.storage_loc(exp, name) {
                    bail!("Expected {:?} but variable \"{}\" was {:?}", exp, name, var_ty);
                } else {
                    expr.ty = Some(exp);
                }
            } else if let Some(var_ty) =  valloc.storage_ty(name) {
                expr.ty = Some(var_ty);
            }
        },
        GetGlobal(name) => {
            if let Some(exp) = expected {
                if let Err(var_ty) =  valloc.global_loc(exp, name) {
                    bail!("Expected {:?} but global \"{}\" was {:?}", exp, name, var_ty);
                } else {
                    expr.ty = Some(exp);
                }
            } else if let Some(var_ty) =  valloc.global_ty(name) {
                expr.ty = Some(var_ty);
            }
        },
        Boolean(_) => expr.ty = Some(Type::Boolean),
        Integer(_) => expr.ty = Some(Type::Integer),
        Float(_) => expr.ty = Some(Type::Float),
        Not(e) => {
            infer_types(valloc, e, Some(Type::Boolean))?;
            expr.ty = e.ty;
        },
        And(l, r)
        | Or(l, r)
        | Xor(l, r) => {
            infer_types(valloc, l, Some(Type::Boolean))?;
            infer_types(valloc, r, Some(Type::Boolean))?;
            if l.ty != Some(Type::Boolean) || r.ty != Some(Type::Boolean) {
                bail!("Mis-matched types: left: {:?} right: {:?}, want: {:?}", l.ty, r.ty, Type::Boolean);
            }
            expr.ty = Some(Type::Boolean);
        },

        Add(l, r)
        | Sub(l, r)
        | Mul(l, r)
        | Div(l, r)
        | Rem(l, r) => {
            infer_types(valloc, l, expected)?;
            infer_types(valloc, r, l.ty)?;
            if l.ty.is_none() && r.ty.is_some() {
                infer_types(valloc, l, r.ty)?;
            }
            if l.ty != r.ty {
                bail!("Mis-matched types: left: {:?} right: {:?}", l.ty, r.ty);
            }
            expr.ty = l.ty;
        },

        Equal(l, r)
        | NotEqual(l, r)
        | LessEqual(l, r)
        | GreaterEqual(l, r)
        | Less(l, r)
        | Greater(l, r) => {
            infer_types(valloc, l, None)?;
            infer_types(valloc, r, l.ty)?;
            if l.ty.is_none() && r.ty.is_some() {
                infer_types(valloc, l, r.ty)?;
            }
            if l.ty != r.ty {
                bail!("Mis-matched types: left: {:?} right: {:?}", l.ty, r.ty);
            }
            if l.ty.is_none() || r.ty.is_none() {
                bail!("Failed to infer types: left: {:?} right: {:?}", l.ty, r.ty);
            }

            expr.ty = Some(Type::Boolean);
        },

        IntToFloat(e) => {
            infer_types(valloc, e, Some(Type::Integer))?;
            if e.ty != Some(Type::Integer) {
                bail!("Mis-matched types: Expected: {:?} Got: {:?}", Type::Integer, e.ty);
            }
            expr.ty = Some(Type::Float);
        },
        FloatToInt(e) => {
            infer_types(valloc, e, Some(Type::Float))?;
            if e.ty != Some(Type::Float) {
                bail!("Mis-matched types: Expected: {:?} Got: {:?}", Type::Float, e.ty);
            }
            expr.ty = Some(Type::Integer);
        },
    }
    Ok(())
}

/// Tries to optimize away constant expressions
fn optimize_expr(expr: parse::Expr<'_>) -> parse::Expr<'_> {
    use self::parse::ExprInner::*;
    match expr.inner {
        // Bool ops
        Not(e) => {
            let e = optimize_expr(*e).inner;
            if let Boolean(b) = e {
                Boolean(!b)
            } else {
                Not(Box::new(e.into()))
            }
        },
        And(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Boolean(l), Boolean(r)) => Boolean(l && r),
                (Boolean(l), r) => if l {
                    r
                } else {
                    Boolean(false)
                },
                (l, Boolean(r)) => if r {
                    l
                } else {
                    Boolean(false)
                },
                (l, r) => And(Box::new(l.into()), Box::new(r.into())),
            }
        },
        Or(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Boolean(l), Boolean(r)) => Boolean(l || r),
                (Boolean(l), r) => if l {
                    Boolean(true)
                } else {
                    r
                },
                (l, Boolean(r)) => if r {
                    Boolean(true)
                } else {
                    l
                },
                (l, r) => Or(Box::new(l.into()), Box::new(r.into())),
            }
        },

        // Math
        Add(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Integer(l), Integer(r)) => Integer(l + r),
                (Float(l), Float(r)) => Float(l + r),
                (l, r) => Add(Box::new(l.into()), Box::new(r.into())),
            }
        },
        Sub(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Integer(l), Integer(r)) => Integer(l - r),
                (Float(l), Float(r)) => Float(l - r),
                (l, r) => Sub(Box::new(l.into()), Box::new(r.into())),
            }
        },
        Mul(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Integer(l), Integer(r)) => Integer(l * r),
                (Float(l), Float(r)) => Float(l * r),
                (l, r) => Mul(Box::new(l.into()), Box::new(r.into())),
            }
        },
        Div(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Integer(l), Integer(r)) => Integer(l / r),
                (Float(l), Float(r)) => Float(l / r),
                (l, r) => Div(Box::new(l.into()), Box::new(r.into())),
            }
        },
        Rem(l, r) => {
            let l = optimize_expr(*l).inner;
            let r = optimize_expr(*r).inner;
            match (l, r) {
                (Integer(l), Integer(r)) => Integer(l % r),
                (Float(l), Float(r)) => Float(l % r),
                (l, r) => Rem(Box::new(l.into()), Box::new(r.into())),
            }
        },


        Equal(l, r) => {
            let l = optimize_expr(*l);
            let r = optimize_expr(*r);
            Equal(Box::new(l), Box::new(r))
        },
        NotEqual(l, r) => {
            let l = optimize_expr(*l);
            let r = optimize_expr(*r);
            NotEqual(Box::new(l), Box::new(r))
        },
        LessEqual(l, r) => {
            let l = optimize_expr(*l);
            let r = optimize_expr(*r);
            LessEqual(Box::new(l), Box::new(r))
        },
        GreaterEqual(l, r) => {
            let l = optimize_expr(*l);
            let r = optimize_expr(*r);
            GreaterEqual(Box::new(l), Box::new(r))
        },
        Less(l, r) => {
            let l = optimize_expr(*l);
            let r = optimize_expr(*r);
            Less(Box::new(l), Box::new(r))
        },
        Greater(l, r) => {
            let l = optimize_expr(*l);
            let r = optimize_expr(*r);
            Greater(Box::new(l), Box::new(r))
        },

        IntToFloat(e) => {
            let e = optimize_expr(*e);
            IntToFloat(Box::new(e))
        },
        FloatToInt(e) => {
            let e = optimize_expr(*e);
            FloatToInt(Box::new(e))
        },

        // Everything else
        expr => expr,
    }.into()
}

#[test]
fn test_optimize() {
    macro_rules! opti_test {
        ($input:expr, $output:expr) => {
            let input = parse::parse($input).expect(concat!("Failed to parse input: ", $input));
            let output = parse::parse($output).expect(concat!("Failed to parse output: ", $output));
            assert_eq!(optimize_expr(input), output);
        };
    }

    opti_test!("!true", "false");
    opti_test!("true && true", "true");
    opti_test!("false && true", "false");
    opti_test!("a && true", "a");
    opti_test!("false || a", "a");
    opti_test!("true || a", "true");

    opti_test!("4", "4");
    opti_test!("4+3", "7");
    opti_test!("4*2", "8");
    opti_test!("9.0/2.0", "4.5");
    opti_test!("12%5", "2");

    opti_test!("3*5+3-5/3*5+43", "56");

}