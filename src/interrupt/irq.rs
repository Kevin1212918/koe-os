use super::IntrptGuard;
use crate::common::{InstrPtr, StackPtr};

pub type IrqVector = u8;
pub struct IrqInfo {
    pub errno: usize,
    pub ip: InstrPtr,
    pub sp: StackPtr,
}

/// Top-half irq handling routine.
///
/// This executes in an interrupt disabled context by the kernel irq
/// handler.
pub type Handler = fn(IrqInfo, &IntrptGuard);
