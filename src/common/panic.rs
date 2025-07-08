use core::panic::PanicInfo;

use crate::arch::die;
use crate::common::log;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    log::panic(format_args!(
        "{} at \n{:?}",
        info.message(),
        info.location(),
    ));
    die()
}
