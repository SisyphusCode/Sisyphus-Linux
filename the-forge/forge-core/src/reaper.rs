use libc::{prctl, PR_SET_CHILD_SUBREAPER};
use std::io::Error;

pub fn claim_subreaper_status() -> Result<(), Error> {
    if unsafe { prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(Error::last_os_error());
    }
    Ok(())
}
