//! ```rust
//! use ravif::*;
//! # fn doit(pixels: &[RGBA8], width: usize, height: usize) -> Result<(), Error> {
//! let res = Encoder::new()
//!     .with_quality(70.)
//!     .with_speed(4)
//!     .encode_rgba(Img::new(pixels, width, height))?;
//! std::fs::write("hello.avif", res.avif_file);
//! # Ok(()) }

mod av1encoder;

mod error;
pub use av1encoder::AlphaColorMode;
pub use av1encoder::ColorSpace;
pub use av1encoder::EncodedImage;
pub use av1encoder::Encoder;
pub use error::BoxError;
pub use error::Error;

pub mod load_rgba;
pub use load_rgba::load_rgba;

#[doc(inline)]
pub use rav1e::prelude::MatrixCoefficients;

mod dirtyalpha;

#[doc(no_inline)]
pub use imgref::Img;
#[doc(no_inline)]
pub use rgb::{RGB8, RGBA8};
