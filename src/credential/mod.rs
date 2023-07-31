pub mod error;
mod traits;
mod stack_string;
mod credential;
mod kind;
mod kinds;
mod util;
pub use traits::*;
pub use kind::*;
pub use kinds::*;

mod consts {
    pub const SERVER_SIDE_KEY_LEN: usize = 40;
    pub const RELAY_AUTO_CONFIG_KEY_LEN: usize = 40;
    pub const MOBILE_KEY_LEN: usize = 40;
    pub const CLIENT_SIDE_ID_LEN: usize = 24;
}

