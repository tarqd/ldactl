mod credential;
pub mod error;
mod kind;
mod kinds;

mod traits;
mod util;
pub use kind::*;
pub use kinds::*;
pub use traits::*;

mod consts {
    pub const SERVER_SIDE_KEY_LEN: usize = 40;
    pub const RELAY_AUTO_CONFIG_KEY_LEN: usize = 40;
    pub const MOBILE_KEY_LEN: usize = 40;
    pub const CLIENT_SIDE_ID_LEN: usize = 24;
}
