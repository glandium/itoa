// Copyright 2016 Itoa Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![doc(html_root_url = "https://docs.rs/itoa/0.3.4")]

#![cfg_attr(not(feature = "std"), no_std)]

#![cfg_attr(feature = "i128", feature(i128_type, i128))]

#![cfg_attr(feature = "cargo-clippy", allow(cast_lossless, unreadable_literal))]

#[cfg(feature = "i128")]
mod udiv128;

#[cfg(feature = "std")]
use std::{fmt, io, mem, ptr, slice, str};

#[cfg(not(feature = "std"))]
use core::{fmt, mem, ptr, slice, str};

#[cfg(feature = "std")]
#[inline]
pub fn write<W: io::Write, V: Integer>(wr: W, value: V) -> io::Result<usize> {
    value.write(wr)
}

#[inline]
pub fn fmt<W: fmt::Write, V: Integer>(wr: W, value: V) -> fmt::Result {
    value.fmt(wr)
}

// Seal to prevent downstream implementations of the Integer trait.
mod private {
    pub trait Sealed {}
}

pub trait Integer: private::Sealed {
    #[cfg(feature = "std")]
    fn write<W: io::Write>(self, W) -> io::Result<usize>;

    fn fmt<W: fmt::Write>(self, W) -> fmt::Result;
}

trait IntegerPrivate {
    fn write_to(self, buf: &mut [u8; MAX_LEN]) -> &[u8];
}

const DEC_DIGITS_LUT: &'static[u8] =
    b"0001020304050607080910111213141516171819\
      2021222324252627282930313233343536373839\
      4041424344454647484950515253545556575859\
      6061626364656667686970717273747576777879\
      8081828384858687888990919293949596979899";

const MAX_LEN: usize = 40; // i128::MIN (including minus sign)

// Adaptation of the original implementation at
// https://github.com/rust-lang/rust/blob/b8214dc6c6fc20d0a660fb5700dca9ebf51ebe89/src/libcore/fmt/num.rs#L188-L266
macro_rules! impl_IntegerCommon {
    ($t:ident) => {
        impl Integer for $t {
            #[cfg(feature = "std")]
            fn write<W: io::Write>(self, mut wr: W) -> io::Result<usize> {
                let mut buf = unsafe { mem::uninitialized() };
                let bytes = self.write_to(&mut buf);
                try!(wr.write_all(bytes));
                Ok(bytes.len())
            }

            fn fmt<W: fmt::Write>(self, mut wr: W) -> fmt::Result {
                let mut buf = unsafe { mem::uninitialized() };
                let bytes = self.write_to(&mut buf);
                wr.write_str(unsafe { str::from_utf8_unchecked(bytes) })
            }
        }

        impl private::Sealed for $t {}
    };
}

macro_rules! impl_Integer {
    ($($t:ident),* as $conv_fn:ident) => {$(
        impl_IntegerCommon!($t);

        impl IntegerPrivate for $t {
            #[allow(unused_comparisons)]
            fn write_to(self, buf: &mut [u8; MAX_LEN]) -> &[u8] {
                let is_nonnegative = self >= 0;
                let mut n = if is_nonnegative {
                    self as $conv_fn
                } else {
                    // convert the negative num to positive by summing 1 to it's 2 complement
                    (!(self as $conv_fn)).wrapping_add(1)
                };
                let mut curr = buf.len() as isize;
                let buf_ptr = buf.as_mut_ptr();
                let lut_ptr = DEC_DIGITS_LUT.as_ptr();

                unsafe {
                    // need at least 16 bits for the 4-characters-at-a-time to work.
                    if mem::size_of::<$t>() >= 2 {
                        // eagerly decode 4 characters at a time
                        while n >= 10000 {
                            let rem = (n % 10000) as isize;
                            n /= 10000;

                            let d1 = (rem / 100) << 1;
                            let d2 = (rem % 100) << 1;
                            curr -= 4;
                            ptr::copy_nonoverlapping(lut_ptr.offset(d1), buf_ptr.offset(curr), 2);
                            ptr::copy_nonoverlapping(lut_ptr.offset(d2), buf_ptr.offset(curr + 2), 2);
                        }
                    }

                    // if we reach here numbers are <= 9999, so at most 4 chars long
                    let mut n = n as isize; // possibly reduce 64bit math

                    // decode 2 more chars, if > 2 chars
                    if n >= 100 {
                        let d1 = (n % 100) << 1;
                        n /= 100;
                        curr -= 2;
                        ptr::copy_nonoverlapping(lut_ptr.offset(d1), buf_ptr.offset(curr), 2);
                    }

                    // decode last 1 or 2 chars
                    if n < 10 {
                        curr -= 1;
                        *buf_ptr.offset(curr) = (n as u8) + b'0';
                    } else {
                        let d1 = n << 1;
                        curr -= 2;
                        ptr::copy_nonoverlapping(lut_ptr.offset(d1), buf_ptr.offset(curr), 2);
                    }

                    if !is_nonnegative {
                        curr -= 1;
                        *buf_ptr.offset(curr) = b'-';
                    }
                }

                let len = buf.len() - curr as usize;
                unsafe { slice::from_raw_parts(buf_ptr.offset(curr), len) }
            }
        }
    )*};
}

impl_Integer!(i8, u8, i16, u16, i32, u32 as u32);
impl_Integer!(i64, u64 as u64);
#[cfg(target_pointer_width = "16")]
impl_Integer!(isize, usize as u16);
#[cfg(target_pointer_width = "32")]
impl_Integer!(isize, usize as u32);
#[cfg(target_pointer_width = "64")]
impl_Integer!(isize, usize as u64);

#[cfg(all(feature = "i128"))]
macro_rules! impl_Integer128 {
    ($($t:ident),*) => {$(
        impl_IntegerCommon!($t);

        impl IntegerPrivate for $t {
            #[allow(unused_comparisons)]
            fn write_to(self, buf: &mut [u8; MAX_LEN]) -> &[u8] {
                let is_nonnegative = self >= 0;
                let n = if is_nonnegative {
                    self as u128
                } else {
                    // convert the negative num to positive by summing 1 to it's 2 complement
                    (!(self as u128)).wrapping_add(1)
                };
                let mut curr = buf.len() as isize;
                let buf_ptr = buf.as_mut_ptr();

                unsafe {
                    // Divide by 10^19 which is the highest power less than 2^64.
                    let (n, rem) = udiv128::udivmod_1e19(n);
                    curr -= rem.write_to(buf).len() as isize;

                    if n != 0 {
                        // Memset the base10 leading zeros of rem.
                        let target = buf.len() as isize - 19;
                        ptr::write_bytes(buf_ptr.offset(target), b'0', (curr - target) as usize);
                        curr = target;

                        // Divide by 10^19 again.
                        let (n, rem) = udiv128::udivmod_1e19(n);
                        let buf2 = buf_ptr.offset(curr - buf.len() as isize) as *mut _;
                        curr -= rem.write_to(&mut *buf2).len() as isize;

                        if n != 0 {
                            // Memset the leading zeros.
                            let target = buf.len() as isize - 38;
                            ptr::write_bytes(buf_ptr.offset(target), b'0', (curr - target) as usize);
                            curr = target;

                            // There is at most one digit left
                            // because u128::max / 10^19 / 10^19 is 3.
                            curr -= 1;
                            *buf_ptr.offset(curr) = (n as u8) + b'0';
                        }
                    }

                    if !is_nonnegative {
                        curr -= 1;
                        *buf_ptr.offset(curr) = b'-';
                    }

                    let len = buf.len() - curr as usize;
                    slice::from_raw_parts(buf_ptr.offset(curr), len)
                }
            }
        }
    )*};
}

#[cfg(all(feature = "i128"))]
impl_Integer128!(i128, u128);
