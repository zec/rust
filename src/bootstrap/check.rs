// Copyright 2018 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Implementation of compiling the compiler and standard library, in "check" mode.

use compile::{run_cargo, std_cargo, test_cargo, rustc_cargo, rustc_cargo_env, add_to_sysroot};
use builder::{RunConfig, Builder, ShouldRun, Step};
use tool::{self, prepare_tool_cargo};
use {Compiler, Mode};
use cache::{INTERNER, Interned};
use std::path::PathBuf;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Std {
    pub target: Interned<String>,
}

impl Step for Std {
    type Output = ();
    const DEFAULT: bool = true;

    fn should_run(run: ShouldRun) -> ShouldRun {
        run.all_krates("std")
    }

    fn make_run(run: RunConfig) {
        run.builder.ensure(Std {
            target: run.target,
        });
    }

    fn run(self, builder: &Builder) {
        let target = self.target;
        let compiler = builder.compiler(0, builder.config.build);

        let out_dir = builder.stage_out(compiler, Mode::Libstd);
        builder.clear_if_dirty(&out_dir, &builder.rustc(compiler));

        let mut cargo = builder.cargo(compiler, Mode::Libstd, target, "check");
        std_cargo(builder, &compiler, target, &mut cargo);

        let _folder = builder.fold_output(|| format!("stage{}-std", compiler.stage));
        println!("Checking std artifacts ({} -> {})", &compiler.host, target);
        run_cargo(builder,
                  &mut cargo,
                  &libstd_stamp(builder, compiler, target),
                  true);

        let libdir = builder.sysroot_libdir(compiler, target);
        add_to_sysroot(&builder, &libdir, &libstd_stamp(builder, compiler, target));
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Rustc {
    pub target: Interned<String>,
}

impl Step for Rustc {
    type Output = ();
    const ONLY_HOSTS: bool = true;
    const DEFAULT: bool = true;

    fn should_run(run: ShouldRun) -> ShouldRun {
        run.all_krates("rustc-main")
    }

    fn make_run(run: RunConfig) {
        run.builder.ensure(Rustc {
            target: run.target,
        });
    }

    /// Build the compiler.
    ///
    /// This will build the compiler for a particular stage of the build using
    /// the `compiler` targeting the `target` architecture. The artifacts
    /// created will also be linked into the sysroot directory.
    fn run(self, builder: &Builder) {
        let compiler = builder.compiler(0, builder.config.build);
        let target = self.target;

        let stage_out = builder.stage_out(compiler, Mode::Librustc);
        builder.clear_if_dirty(&stage_out, &libstd_stamp(builder, compiler, target));
        builder.clear_if_dirty(&stage_out, &libtest_stamp(builder, compiler, target));

        let mut cargo = builder.cargo(compiler, Mode::Librustc, target, "check");
        rustc_cargo(builder, &mut cargo);

        let _folder = builder.fold_output(|| format!("stage{}-rustc", compiler.stage));
        println!("Checking compiler artifacts ({} -> {})", &compiler.host, target);
        run_cargo(builder,
                  &mut cargo,
                  &librustc_stamp(builder, compiler, target),
                  true);

        let libdir = builder.sysroot_libdir(compiler, target);
        add_to_sysroot(&builder, &libdir, &librustc_stamp(builder, compiler, target));
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct CodegenBackend {
    pub target: Interned<String>,
    pub backend: Interned<String>,
}

impl Step for CodegenBackend {
    type Output = ();
    const ONLY_HOSTS: bool = true;
    const DEFAULT: bool = true;

    fn should_run(run: ShouldRun) -> ShouldRun {
        run.all_krates("rustc_codegen_llvm")
    }

    fn make_run(run: RunConfig) {
        let backend = run.builder.config.rust_codegen_backends.get(0);
        let backend = backend.cloned().unwrap_or_else(|| {
            INTERNER.intern_str("llvm")
        });
        run.builder.ensure(CodegenBackend {
            target: run.target,
            backend,
        });
    }

    fn run(self, builder: &Builder) {
        let compiler = builder.compiler(0, builder.config.build);
        let target = self.target;
        let backend = self.backend;

        let mut cargo = builder.cargo(compiler, Mode::Librustc, target, "check");
        let features = builder.rustc_features().to_string();
        cargo.arg("--manifest-path").arg(builder.src.join("src/librustc_codegen_llvm/Cargo.toml"));
        rustc_cargo_env(builder, &mut cargo);

        // We won't build LLVM if it's not available, as it shouldn't affect `check`.

        let _folder = builder.fold_output(|| format!("stage{}-rustc_codegen_llvm", compiler.stage));
        run_cargo(builder,
                  cargo.arg("--features").arg(features),
                  &codegen_backend_stamp(builder, compiler, target, backend),
                  true);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Test {
    pub target: Interned<String>,
}

impl Step for Test {
    type Output = ();
    const DEFAULT: bool = true;

    fn should_run(run: ShouldRun) -> ShouldRun {
        run.all_krates("test")
    }

    fn make_run(run: RunConfig) {
        run.builder.ensure(Test {
            target: run.target,
        });
    }

    fn run(self, builder: &Builder) {
        let compiler = builder.compiler(0, builder.config.build);
        let target = self.target;

        let out_dir = builder.stage_out(compiler, Mode::Libtest);
        builder.clear_if_dirty(&out_dir, &libstd_stamp(builder, compiler, target));

        let mut cargo = builder.cargo(compiler, Mode::Libtest, target, "check");
        test_cargo(builder, &compiler, target, &mut cargo);

        let _folder = builder.fold_output(|| format!("stage{}-test", compiler.stage));
        println!("Checking test artifacts ({} -> {})", &compiler.host, target);
        run_cargo(builder,
                  &mut cargo,
                  &libtest_stamp(builder, compiler, target),
                  true);

        let libdir = builder.sysroot_libdir(compiler, target);
        add_to_sysroot(builder, &libdir, &libtest_stamp(builder, compiler, target));
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Rustdoc {
    pub target: Interned<String>,
}

impl Step for Rustdoc {
    type Output = ();
    const ONLY_HOSTS: bool = true;
    const DEFAULT: bool = true;

    fn should_run(run: ShouldRun) -> ShouldRun {
        run.path("src/tools/rustdoc")
    }

    fn make_run(run: RunConfig) {
        run.builder.ensure(Rustdoc {
            target: run.target,
        });
    }

    fn run(self, builder: &Builder) {
        let compiler = builder.compiler(0, builder.config.build);
        let target = self.target;

        let mut cargo = prepare_tool_cargo(builder,
                                           compiler,
                                           target,
                                           "check",
                                           "src/tools/rustdoc");

        let _folder = builder.fold_output(|| format!("stage{}-rustdoc", compiler.stage));
        println!("Checking rustdoc artifacts ({} -> {})", &compiler.host, target);
        run_cargo(builder,
                  &mut cargo,
                  &rustdoc_stamp(builder, compiler, target),
                  true);

        let libdir = builder.sysroot_libdir(compiler, target);
        add_to_sysroot(&builder, &libdir, &rustdoc_stamp(builder, compiler, target));

        builder.ensure(tool::CleanTools {
            compiler,
            target,
            mode: Mode::Tool,
        });
    }
}

/// Cargo's output path for the standard library in a given stage, compiled
/// by a particular compiler for the specified target.
pub fn libstd_stamp(builder: &Builder, compiler: Compiler, target: Interned<String>) -> PathBuf {
    builder.cargo_out(compiler, Mode::Libstd, target).join(".libstd-check.stamp")
}

/// Cargo's output path for libtest in a given stage, compiled by a particular
/// compiler for the specified target.
pub fn libtest_stamp(builder: &Builder, compiler: Compiler, target: Interned<String>) -> PathBuf {
    builder.cargo_out(compiler, Mode::Libtest, target).join(".libtest-check.stamp")
}

/// Cargo's output path for librustc in a given stage, compiled by a particular
/// compiler for the specified target.
pub fn librustc_stamp(builder: &Builder, compiler: Compiler, target: Interned<String>) -> PathBuf {
    builder.cargo_out(compiler, Mode::Librustc, target).join(".librustc-check.stamp")
}

/// Cargo's output path for librustc_codegen_llvm in a given stage, compiled by a particular
/// compiler for the specified target and backend.
fn codegen_backend_stamp(builder: &Builder,
                         compiler: Compiler,
                         target: Interned<String>,
                         backend: Interned<String>) -> PathBuf {
    builder.cargo_out(compiler, Mode::Librustc, target)
         .join(format!(".librustc_codegen_llvm-{}-check.stamp", backend))
}

/// Cargo's output path for rustdoc in a given stage, compiled by a particular
/// compiler for the specified target.
pub fn rustdoc_stamp(builder: &Builder, compiler: Compiler, target: Interned<String>) -> PathBuf {
    builder.cargo_out(compiler, Mode::Tool, target).join(".rustdoc-check.stamp")
}
