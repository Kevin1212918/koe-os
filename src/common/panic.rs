use core::fmt::Write as _;
use core::panic::PanicInfo;

use crate::common::{hlt, log};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    log::panic(format_args!(
        "{} at \n{:?}",
        info.message(),
        info.location(),
    ));
    hlt()
}
