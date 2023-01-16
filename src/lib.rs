#![no_std]

//! This crate provides the [`asm_block!`] macro for allowing composition
//! through Rust macro when writing inline assembly.
//!
//! [`asm!`] in Rust accepts a template string as input. While it automatically
//! add `\n` between comma-separated strings, it relies solely on the assembler
//! macros to build composable assembly. While it is fine in most cases, it
//! becomes a problem when we want to use the same macro across functions.
//!
//! # Motivation
//! Consider the following code using `x86_64` assembly:
//! ```no_run
//! # use std::arch::asm;
//! # #[cfg(target_arch = "x86_64")]
//! unsafe fn f() -> u64 {
//!     let mut x = 20;
//!     asm!(
//!         ".macro mad x, y",
//!         "  mul x, y",
//!         "  lea x, [x + y]",
//!         ".endm",
//!         "mad {x}, 5",
//!         
//!         x = inout(reg) x
//!     );
//!     x
//! }
//! # unsafe { println!("{}", f()) };
//! ```
//! If we want to reuse `mad` in another function, we must copy the verbatim
//! of the macro and change its name. Otherwise we will encounter compilation
//! error due to name collision.
//! ```compile_fail
//! # use std::arch::asm;
//! # #[cfg(target_arch = "x86_64")]
//! unsafe fn f() -> u64 {
//!     let mut x = 20;
//!     asm!(
//!         ".macro mad x, y",
//!         "  mul x, y",
//!         "  lea x, [x + y]",
//!         ".endm",
//!         "mad {x}, 5",
//!         
//!         x = inout(reg) x
//!     );
//!     x
//! }
//!
//! # #[cfg(target_arch = "x86_64")]
//! unsafe fn g() -> u64 {
//!     let mut x = 10;
//!     asm!(
//!         // Only compiles if we remove this macro definition
//!         // or rename it to another name
//!         ".macro mad x, y",
//!         "  mul x, y",
//!         "  lea x, [x + y]",
//!         ".endm",
//!         "mad {x}, 8",
//!         
//!         x = inout(reg) x
//!     );
//!     x
//! }
//! # unsafe { println!("{} {}", f(), g()) };
//! # // Make it fails to compile on other target
//! # #[cfg(not(target_arch = "x86_64"))]
//! # this_should_fail_to_compile
//! ```
//! The above code fails with
//! ```text
//! error: macro 'mad' is already defined
//! ```
//! If we omit the definition of `mad` in `g()`, it will compile, but only
//! when `g()` is emitted after `f()`. It is unclear which function should house
//! the definition, so the only sane option is to house it in a `global_asm!`
//! code. But again, it is hard to guarantee that the definition is emitted
//! before the actual use.
//!
//! It is natural to resort to Rust macro in this case, but due to the fact that
//! [`asm!`] accepts a template string, substituting metavariables becomes
//! tedious.
//! ```no_run
//! # use std::arch::asm;
//! macro_rules! mad {
//!     ($x: ident, $y: literal) => {
//!         concat!(
//!             "mul {", stringify!($x), "}, ", stringify!($y), "\n",
//!             "lea {", stringify!($x), "}, [{", stringify!($x), "}+", stringify!($y), "]"
//!         )
//!     };
//! }
//! # #[cfg(target_arch = "x86_64")]
//! unsafe fn f() -> u64 {
//!     let mut x = 20;
//!     asm!(
//!         mad!(x, 5),
//!         
//!         x = inout(reg) x
//!     );
//!     x
//! }
//! # unsafe { println!("{}", f()) };
//! ```
//! This approach has some multiple drawbacks:
//! - The definition is very noisy, making it hard to read and comprehend. It is
//!   much worse if the definition becomes longer, and much much worse if
//!   `rustfmt` attempts to format it.
//! - It is easy to forget `,` and `\n` when the definition becomes longer.
//! - `mad!` can only accept a named register as the first argument and a
//!   literal as the second argument. We cannot call `mad!(x, rbx)` or
//!   `mad!([rax], rbp)`, which we would have been able to if we were using the
//!   assembler macro. Trying to fix this by changing `ident` and `literal` to
//!   `tt` is also problematic, since `stringify!({x})` becomes `"{ x }"`, and
//!   it is an invalid placeholder.
//!
//! This crate tries to address this by providing a macro that makes it easier
//! to compose assembly code.
//!
//! # Example
//! Instead of the code above, using [`asm_block!`], we are able to write the
//! following:
//! ```no_run
//! # use std::arch::asm;
//! use asm_block::asm_block;
//! macro_rules! mad {
//!     ($x: tt, $y: tt) => {
//!         asm_block! {
//!             mul $x, $y;
//!             lea $x, [$x + $y];
//!         }
//!     };
//! }
//! # #[cfg(target_arch = "x86_64")]
//! #[rustfmt::skip::macros(mad)]
//! unsafe fn f() -> u64 {
//!     let mut x = 20;
//!     asm!(
//!         mad!({x}, 5),
//!         
//!         x = inout(reg) x
//!     );
//!     x
//! }
//! # unsafe { println!("{}", f()) };
//! ```
//! Now we are able to make calls like `mad!({x}, rbx)`, `mad!([rax], rbp)`, and
//! `mad!({x:e}, [rsp - 4])`.
//!
//! # Limitations
//! - Due to the tokenization rule of Rust macro, strings enclosed by `'` are
//!   not supported.
//! - [`asm_block!`] mostly consumes tokens one by one, so it is possible to run
//!   out of recursion limit if the assembly code is long. User needs
//!   `#![recursion_limit = "<a_larger_value>"]` when encountering the error.
//! - `rustfmt` will format `mad!({x}, 5)` into `mad!({ x }, 5)`. While this
//!   won't make any difference in the emitted assembly code, it is confusing to
//!   read when the user is expecting a format placeholder. User can use
//!   `#[rustfmt::skip::macros(mad)]` to prevent `rustfmt` from formatting the
//!   interior of `mad!` calls.
//! - Some assemblers use `;` as the comment starter, but we are using it as
//!   instruction delimeter, so assembly comments may not work properly. Users
//!   are strongly suggested to stick to Rust comments.
//! - `tt` cannot capture multiple tokens, so to make `mad!(dword ptr [rax],
//!   ebp)` possible, calling convention of `mad!` needs to be changed. For
//!   example
//!   ```no_run
//!   use asm_block::asm_block;
//!   macro_rules! mad {
//!       ([{ $($x: tt)+ }], $y: tt) => {
//!           asm_block! {
//!               mul $($x)+, $y;
//!               lea $($x)+, [$($x)+ + $y];
//!           }
//!       };
//!       ($x: tt, $y: tt) => { mad!([{ $x }], $y) };
//!   }
//!   # #[cfg(target_arch = "x86_64")]
//!   # unsafe {
//!   #     use std::arch::asm;
//!   #     asm!(
//!   #         mad!([{ dword ptr [{x}] }], ebp),
//!   #         
//!   #         x = out(reg) _
//!   #     );
//!   # }
//!   ```
//!   But `mad!` must be called with `mad!([{ dword ptr [rax] }], ebp)` instead.
//! - Currently we don't have an escape hatch to manually inject assembly if the
//!   macro is not able to emit the correct assembly code.
//!
//! # License
//! Dual licensed under the Apache 2.0 license and the MIT license.
//!
//! [`asm_block!`]: macro.asm_block.html
//! [`asm!`]: https://doc.rust-lang.org/stable/core/arch/macro.asm.html

/// Translate tokens to a string containing assembly.
/// 
/// This evaluates to a `&'static str`. Most input should be transformed as-is in to a
/// string, but there will likely be extra whitespaces or shrunken whitespaces.
/// 
/// # How it Works
/// This macro follows very simple rules and mostly relies on the whitespace leniency
/// of the underlying assembler.
/// 
/// Transformation rules:
/// - Convert `;` to `\n`.
/// - No space before and after `@`, `:`.
/// - Must have a space after `.<ident>`.
/// - Not violating the previous rule, no space before `.`.
/// - Concatenate everything inside a pair of `{` and `}` without any space.
/// - Transcribe all the other tokens as-is (by `stringify!`), and add a space afterwards.
/// 
/// This should work for most assembly code.
/// 
/// # Example
/// ```no_run
/// # use std::arch::asm;
/// use asm_block::asm_block;
/// macro_rules! f {
///     ($a: tt, $b: tt, $c: tt, $d: tt, $k: tt, $s: literal, $t: literal, $tmp: tt) => {
///         asm_block! {
///             mov $tmp, $c;
///             add $a, $k;
///             xor $tmp, $d;
///             and $tmp, $b;
///             xor $tmp, $d;
///             lea $a, [$a + $tmp + $t];
///             rol $a, $s;
///             add $a, $b;
///         }
///     };
/// }
/// 
/// # #[cfg(target_arch = "x86_64")]
/// # unsafe {
/// asm!(
///     f!(eax, ebx, ecx, edx, [ebp + 4], 7, 0xd76aa478, esi),
///     f!({a}, {b}, {c}, {d}, {x0}, 7, 0xd76aa478, {t}),
///     f!({a:e}, {b:e}, {c:e}, {d:e}, [{x} + 4], 7, 0xd76aa478, {t:e}),
///     
///     a = out(reg) _,
///     b = out(reg) _,
///     c = out(reg) _,
///     d = out(reg) _,
///     x0 = out(reg) _,
///     x = out(reg) _,
///     t = out(reg) _,
/// );
/// # }
/// ```
#[allow(clippy::deprecated_cfg_attr)]
#[cfg_attr(rustfmt, rustfmt::skip)]
#[macro_export]
macro_rules! asm_block {
    // base case
    () => { "" };

    // convert `;` to newline
    (; $($token: tt)*) => {
        concat!("\n", $crate::asm_block!($($token)*))
    };

    // no space between an `ident` and a `:`
    ($first: ident : $($token: tt)*) => {
        concat!(stringify!($first), $crate::asm_block!(: $($token)*))
    };

    // no space between an `ident` and a `@`
    ($first: ident @ $($token: tt)*) => {
        concat!(stringify!($first), $crate::asm_block!(@ $($token)*))
    };

    // no space between an `ident` and a `.`
    ($first: ident . $($token: tt)*) => {
        concat!(stringify!($first), $crate::asm_block!(. $($token)*))
    };

    // no space after `:`, `@`
    (: $($token: tt)*) => {
        concat!(":", $crate::asm_block!($($token)*))
    };
    (@ $($token: tt)*) => {
        concat!("@", $crate::asm_block!($($token)*))
    };

    // must have a space after `.<tt>`
    (. $first: tt $($token: tt)*) => {
        concat!(".", stringify!($first), " ", $crate::asm_block!($($token)*))
    };

    // stringify inside {} and ''
    ({$($token_inside: tt)*} $($token: tt)*) => {
        concat!("{", $(stringify!($token_inside),)* "}", $crate::asm_block!($($token)*))
    };

    // expand `[]` and `()`
    ([$($token_inside: tt)*] $($token: tt)*) => {
        concat!("[", $crate::asm_block!($($token_inside)*), "] ", $crate::asm_block!($($token)*))
    };
    (($($token_inside: tt)*) $($token: tt)*) => {
        concat!("(", $crate::asm_block!($($token_inside)*), ") ", $crate::asm_block!($($token)*))
    };

    // For all other type of tokens, add a space after
    ($first: tt $($token: tt)*) => {
        concat!(stringify!($first), " ", $crate::asm_block!($($token)*))
    };
}

#[cfg(test)]
#[rustfmt::skip::macros(asm_block)]
mod tests {
    #[test]
    fn test_single_item() {
        assert_eq!(asm_block!(), "");
        assert_eq!(asm_block!(eax), "eax ");
        assert_eq!(asm_block!(mov), "mov ");
        assert_eq!(asm_block!(_WriteConsoleA@20), "_WriteConsoleA@20 ");
        assert_eq!(asm_block!(@20), "@20 ");
        assert_eq!(asm_block!(@a), "@a ");
        assert_eq!(asm_block!(%0), "% 0 ");
        assert_eq!(asm_block!(%a), "% a ");
        assert_eq!(asm_block!(%a@0), "% a@0 ");
        assert_eq!(asm_block!(%{a}), "% {a}");
        assert_eq!(asm_block!(#0), "# 0 ");
        assert_eq!(asm_block!(#a), "# a ");
        assert_eq!(asm_block!(#a_0), "# a_0 ");
        assert_eq!(asm_block!(.0), ".0 ");
        assert_eq!(asm_block!(.a), ".a ");
        assert_eq!(asm_block!(.a_0), ".a_0 ");
        assert_eq!(asm_block!("$a"), r#""$a" "#);
        assert_eq!(asm_block!(${a}), "$ {a}");
        assert_eq!(asm_block!(${a:e}), "$ {a:e}");
        assert_eq!(asm_block!(v19.4s), "v19.4s ");
        assert_eq!(asm_block!(v1.4s), "v1.4s ");
        assert_eq!(asm_block!({x:v}.4s), "{x:v}.4s ");
        assert_eq!(asm_block!(a), "a ");
        assert_eq!(asm_block!(A), "A ");
        assert_eq!(asm_block!(0), "0 ");
        assert_eq!(asm_block!(0x1234), "0x1234 ");
        assert_eq!(asm_block!(-0x1234), "- 0x1234 ");
        assert_eq!(
            asm_block!(gs:[eax + 4*{b:e} - 0x30]),
            "gs:[eax + 4 * {b:e}- 0x30 ] "
        );
        assert_eq!(asm_block!(%gs:4(,%eax,8)), "% gs:4 (, % eax , 8 ) ");
    }

    #[test]
    fn test_single_instruction() {
        assert_eq!(asm_block!(mov {x}, [{x}]), "mov {x}, [{x}] ");
        assert_eq!(asm_block!(inc), "inc ");
        assert_eq!(asm_block!(_start: mov rax, 1), "_start:mov rax , 1 ");
        assert_eq!(asm_block!(mov $1, %rax), "mov $ 1 , % rax ");
        assert_eq!(asm_block!(.section .text), ".section .text ");
        assert_eq!(asm_block!(L001:), "L001:");
        assert_eq!(
            asm_block!(pushl %fs:table(%ebx, %ecx, 8)),
            "pushl % fs:table (% ebx , % ecx , 8 ) "
        );
        assert_eq!(
            asm_block!(message:  db        "Hello, World", 10),
            r#"message:db "Hello, World" , 10 "#
        );
        assert_eq!(
            asm_block!(.ascii  "Hello, world\n"),
            r#".ascii "Hello, world\n" "#
        );
        assert_eq!(
            asm_block!(call    _WriteConsoleA@20),
            "call _WriteConsoleA@20 "
        );
        assert_eq!(asm_block!(str  fp, [sp, -4]!), "str fp , [sp , - 4 ] ! ");
        assert_eq!(asm_block!(ldr fp, [{x}], 4), "ldr fp , [{x}] , 4 ");
        assert_eq!(
            asm_block!(add   v19.4s, v2.4s, v4.4s),
            "add v19.4s , v2.4s , v4.4s "
        );
    }

    #[test]
    fn test_block() {
        assert_eq!(
            asm_block! {
                push 0;
                push offset written;
                push 13;
                push offset msg;
                push handle;
                call _WriteConsoleA@20;
            },
            "\
push 0 
push offset written 
push 13 
push offset msg 
push handle 
call _WriteConsoleA@20 
"
        );
        assert_eq!(
            asm_block! {
                mov {t1:e}, {d:e};
                not {t1:e};
                add {a:e}, {k:e};
                or {t1:e}, {b:e};
                xor {t1:e}, {c:e};
                lea {a:e}, [{a:e} + {t1:e} + 0xf4d50d87];
                rol {a:e}, 7;
                add {a:e}, {b:e};
            },
            "\
mov {t1:e}, {d:e}
not {t1:e}
add {a:e}, {k:e}
or {t1:e}, {b:e}
xor {t1:e}, {c:e}
lea {a:e}, [{a:e}+ {t1:e}+ 0xf4d50d87 ] 
rol {a:e}, 7 
add {a:e}, {b:e}
"
        );
    }

    #[test]
    #[rustfmt::skip::macros(f)]
    fn test_substitute() {
        macro_rules! f {
            ($a: tt, $b: tt, $c: tt, $d: tt, $k: tt, $s: literal, $t: literal, $tmp: tt) => {
                asm_block! {
                    mov $tmp, $c;
                    add $a, $k;
                    xor $tmp, $d;
                    and $tmp, $b;
                    xor $tmp, $d;
                    lea $a, [$a + $tmp + $t];
                    rol $a, $s;
                    add $a, $b;
                }
            };
        }

        assert_eq!(
            f!({a}, {b}, {c}, {d}, {x0}, 7, 0xd76aa478, {t1}),
            "\
mov {t1}, {c}
add {a}, {x0}
xor {t1}, {d}
and {t1}, {b}
xor {t1}, {d}
lea {a}, [{a}+ {t1}+ 0xd76aa478 ] 
rol {a}, 7 
add {a}, {b}
"
        );

        assert_eq!(
            f!({a:e}, {b:e}, {c:e}, {d:e}, {x0:e}, 7, 0xd76aa478, {t1:e}),
            "\
mov {t1:e}, {c:e}
add {a:e}, {x0:e}
xor {t1:e}, {d:e}
and {t1:e}, {b:e}
xor {t1:e}, {d:e}
lea {a:e}, [{a:e}+ {t1:e}+ 0xd76aa478 ] 
rol {a:e}, 7 
add {a:e}, {b:e}
"
        );

        assert_eq!(
            f!(eax, ebx, ecx, edx, [ebp + 4], 7, 0xd76aa478, esi),
            "\
mov esi , ecx 
add eax , [ebp + 4 ] 
xor esi , edx 
and esi , ebx 
xor esi , edx 
lea eax , [eax + esi + 0xd76aa478 ] 
rol eax , 7 
add eax , ebx 
"
        );
    }
}
