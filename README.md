Assembly Block
==============

[![crate][crate-image]][crate-link]
[![Docs][docs-image]][docs-link]
![Apache2/MIT licensed][license-image]
[![Build Status][build-image]][build-link]

This crate provides a macro to translate tokens into a string which mostly works in [`core::arch::asm!`] macro.

# Add
```sh
cargo add asm_block
```

# Example
```rust
use asm_block::asm_block;

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

asm!(
    f!(eax, ebx, ecx, edx, [ebp + 4], 7, 0xd76aa478, esi),
    f!({a}, {b}, {c}, {d}, {x0}, 7, 0xd76aa478, {t}),
    f!({a:e}, {b:e}, {c:e}, {d:e}, [{x} + 4], 7, 0xd76aa478, {t:e}),
    
    a = out(reg) _,
    b = out(reg) _,
    c = out(reg) _,
    d = out(reg) _,
    x0 = out(reg) _,
    x = out(reg) _,
    t = out(reg) _,
);
```

# Design
`asm_block` follows very simple rules and mostly relies on the whitespace leniency
of the underlying assembler.

Transformation rules:
- Convert `;` to `\n`.
- No space before `:`.
- No space after `.`.
- No space before and after `@`.
- Concatenate everything inside a pair of `{` and `}` without any space.
- Transcribe all the other tokens as-is (by `stringify!`), and add a space afterwards.

This should work for most assembly code. We have checked that space after `$`, `#`, `!`, `%`, `:`, `=` won't invalidate an assembly using `x86_64` target and `aarch64` target.

# Motivation
Consider the following code using `x86_64` assembly:
```rust
unsafe fn f() -> u64 {
    let mut x = 20;
    asm!(
        ".macro mad x, y",
        "  mul x, y",
        "  lea x, [x + y]",
        ".endm",
        "mad {x}, 5",
        
        x = inout(reg) x
    );
    x
}
```
If we want to reuse `mad!` in another function, we must copy the verbatim
of the macro and change its name. Otherwise we will encounter compilation
error due to name collision.
```rust
unsafe fn f() -> u64 {
    let mut x = 20;
    asm!(
        ".macro mad x, y",
        "  mul x, y",
        "  lea x, [x + y]",
        ".endm",
        "mad {x}, 5",
        
        x = inout(reg) x
    );
    x
}

unsafe fn g() -> u64 {
    let mut x = 10;
    asm!(
        // Only compiles if we remove this macro definition
        // or rename it to another name
        ".macro mad x, y",
        "  mul x, y",
        "  lea x, [x + y]",
        ".endm",
        "mad {x}, 8",
        
        x = inout(reg) x
    );
    x
}
```
The above code fails with
```text
error: macro 'mad' is already defined
```
If we omit the definition of `mad!` in `g()`, it will compile, but only
when `g()` is emitted after `f()`. It is unclear which function should house
the definition, so the only sane option is to house it in a `global_asm!`
code. But again, it is hard to guarantee that the definition is emitted
before the actual use.

It is natural to resort to Rust macro in this case, but due to the fact that
[`asm!`] accepts a template string, substituting metavariables becomes
tedious.
```rust
macro_rules! mad {
    ($x: ident, $y: literal) => {
        concat!(
            "mul {", stringify!($x), "}, ", stringify!($y), "\n",
            "lea {", stringify!($x), "}, [{", stringify!($x), "}+", stringify!($y), "]"
        )
    };
}

unsafe fn f() -> u64 {
    let mut x = 20;
    asm!(
        mad!(x, 5),
        
        x = inout(reg) x
    );
    x
}
```
This approach has some multiple drawbacks:
- The definition is very noisy, making it hard to read and comprehend. It is
  much worse if the definition becomes longer, and much much worse if
  `rustfmt` attempts to format it.
- It is easy to forget `,` and `\n` when the definition becomes longer.
- `mad!` can only accept a named register as the first argument and a
  literal as the second argument. We cannot call `mad!(x, rbx)` or
  `mad!([rax], rbp)`, which we would have been able to if we were using the
  assembler macro. Trying to fix this by changing `ident` and `literal` to
  `tt` is also problematic, since `stringify!({x})` becomes `"{ x }"`, and
  it is an invalid placeholder.

This crate tries to address this by providing a macro that makes it easier
to compose assembly code.

```rust
use asm_block::asm_block;

macro_rules! mad {
    ($x: tt, $y: tt) => {
        asm_block! {
            mul $x, $y;
            lea $x, [$x + $y];
        }
    };
}

#[rustfmt::skip::macros(mad)]
unsafe fn f() -> u64 {
    let mut x = 20;
    asm!(
        mad!({x}, 5),
        
        x = inout(reg) x
    );
    x
}
```
Now we are able to make calls like `mad!({x}, rbx)`, `mad!([rax], rbp)`, and
`mad!({x:e}, [rsp - 4])`. And this looks much cleaner.


# Limitations
- Due to the tokenization rule of Rust macro, strings enclosed by `'` are
  not supported.
- [`asm_block!`] mostly consumes tokens one by one, so it is possible to run
  out of recursion limit if the assembly code is long. User needs
  `#![recursion_limit = "<a_larger_value>"]` when encountering the error.
- `rustfmt` will format `mad!({x}, 5)` into `mad!({ x }, 5)`. While this
  won't make any difference in the emitted assembly code, it is confusing to
  read when the user is expecting a format placeholder. User can use
  `#[rustfmt::skip::macros(mad)]` to prevent `rustfmt` from formatting the
  interior of `mad!` calls.
- Some assemblers use `;` as the comment starter, but we are using it as
  instruction delimeter, so assembly comments may not work properly. Users
  are strongly suggested to stick to Rust comments.
- `tt` cannot capture multiple tokens, so to make `mad!(dword ptr [rax],
  ebp)` possible, calling convention of `mad!` needs to be changed. For
  example
  ```rust
  use asm_block::asm_block;

  macro_rules! mad {
      ([{ $($x: tt)+ }], $y: tt) => {
          asm_block! {
              mul $($x: tt)+, $y;
              lea $($x: tt)+, [$($x: tt)+ + $y];
          }
      };
      ($x: tt, $y: tt) => { mad!([{ $x }], $y) };
  }
  ```
  But `mad!` must be called with `mad!([{ dword ptr [rax] }], ebp)` instead.
- Currently we don't have an escape hatch to manually inject assembly if the
  macro is not able to emit the correct assembly code.

# License

Licensed under either of:

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

at your option.


[//]: # (badges and links)

[crate-image]: https://img.shields.io/crates/v/asm_block.svg
[crate-link]: https://crates.io/crates/asm_block
[docs-image]: https://docs.rs/asm_block/badge.svg
[docs-link]: https://docs.rs/asm_block/
[license-image]: https://img.shields.io/badge/license-Apache2.0/MIT-blue.svg
[build-image]: https://github.com/johnmave126/asm_block/actions/workflows/main.yml/badge.svg?branch=master&event=push
[build-link]: https://github.com/johnmave126/asm_block/actions/workflows/main.yml?query=branch:master

[`core::arch::asm!`]: https://doc.rust-lang.org/stable/core/arch/macro.asm.html