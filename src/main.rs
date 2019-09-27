#![feature(rustc_private)]
#![deny(rust_2018_idioms)]

#[macro_use]
extern crate log;

#[macro_use]
extern crate rustc;
extern crate rustc_driver;
extern crate rustc_interface;

mod init;
mod petri_net;
mod translator;

use crate::translator::Translator;
use rustc::hir::def_id::LOCAL_CRATE;
use rustc_driver::Compilation;
use rustc_interface::interface;

struct PetriConfig {
    _arguments: Vec<String>,
}

impl rustc_driver::Callbacks for PetriConfig {
    fn after_analysis(&mut self, compiler: &interface::Compiler) -> Compilation {
        init::init_late_loggers();
        compiler.session().abort_if_errors();

        compiler.global_ctxt().unwrap().peek_mut().enter(|tcx| {
            let (entry_def_id, _) = tcx.entry_fn(LOCAL_CRATE).expect("no main function found!");
            let mut pass = Translator::new(tcx);
            pass.translate(entry_def_id);
        });

        compiler.session().abort_if_errors();

        Compilation::Stop
    }
}
pub fn main() {
    init::init_early_loggers();
    let (mut rustc_args, fairum_args) = init::parse_arguments();
    init::check_sysroot(&mut rustc_args);

    let mut config = PetriConfig {
        _arguments: fairum_args,
    };
    let result = rustc_driver::report_ices_to_stderr_if_any(move || {
        rustc_driver::run_compiler(&rustc_args, &mut config, None, None)
    })
    .and_then(|result| result);
    std::process::exit(result.is_err() as i32);
}