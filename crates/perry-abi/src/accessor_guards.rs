//! The accessor answer's guard program, shared by runtime evaluation and
//! LLVM emission. Backends may discharge a guard with a dominating proof
//! about the same entry/receiver, with no mutation or collecting edge between.
//!
//! `callable` also establishes worker exclusion. For an inline consumer it
//! must prove a nonzero getter and an inline lane before `lane` loads it.
//! A collecting consumer permits setter-only and spill answers; it derives a
//! spill getter only after the current lane has matched the rooted pair.

pub trait AccessorGuards {
    type Failure;
    fn receiver(&mut self) -> Result<(), Self::Failure>;
    fn kind(&mut self) -> Result<(), Self::Failure>;
    fn holder(&mut self) -> Result<(), Self::Failure>;
    fn callable(&mut self) -> Result<(), Self::Failure>;
    fn lane(&mut self) -> Result<(), Self::Failure>;
}

#[inline(always)]
pub fn validate<G: AccessorGuards>(guards: &mut G) -> Result<(), G::Failure> {
    guards.receiver()?;
    guards.kind()?;
    guards.holder()?;
    guards.callable()?;
    guards.lane()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe {
        fail: usize,
        visited: usize,
    }

    impl Probe {
        fn guard(&mut self) -> Result<(), usize> {
            let index = self.visited;
            self.visited += 1;
            if index == self.fail {
                Err(index)
            } else {
                Ok(())
            }
        }
    }

    impl AccessorGuards for Probe {
        type Failure = usize;
        fn receiver(&mut self) -> Result<(), usize> {
            self.guard()
        }
        fn kind(&mut self) -> Result<(), usize> {
            self.guard()
        }
        fn holder(&mut self) -> Result<(), usize> {
            self.guard()
        }
        fn callable(&mut self) -> Result<(), usize> {
            self.guard()
        }
        fn lane(&mut self) -> Result<(), usize> {
            self.guard()
        }
    }

    #[test]
    fn every_failed_guard_declines_before_later_loads() {
        for fail in 0..5 {
            let mut p = Probe { fail, visited: 0 };
            assert_eq!(validate(&mut p), Err(fail));
            assert_eq!(p.visited, fail + 1);
        }
        let mut p = Probe {
            fail: 5,
            visited: 0,
        };
        assert_eq!(validate(&mut p), Ok(()));
        assert_eq!(p.visited, 5);
    }
}
