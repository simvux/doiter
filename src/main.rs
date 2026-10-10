use std::{io::Read, path::PathBuf};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, registry::Registry};

mod engine;
mod error;
mod lexer;
mod parser;
mod type_check;

struct Options {
    cleanup: bool,
    cleanup_all: bool,

    print_ast: bool,

    file: Option<PathBuf>,
}

impl Options {
    fn parse() -> Self {
        use bpaf::*;
        use bpaf::{Parser, ShellComp};

        let cleanup = long("no-cleanup")
            .help("Remove docker containers after execution from this doiter program")
            .flag(false, true);

        let cleanup_all = long("cleanup-all")
            .help("Remove all docker containers created by doiter")
            .flag(true, false);

        let print_ast = long("print-ast")
            .help("Print the AST and exit")
            .flag(true, false);

        let file = positional::<PathBuf>("FILE")
            .help("doiter (`.dr`) input file")
            .optional()
            .complete_shell(ShellComp::File {
                mask: Some("*(dr)"),
            });

        construct!(Options {
            cleanup,
            cleanup_all,
            print_ast,
            file
        })
        .to_options()
        .run()
    }
}

fn logger() {
    let filter = EnvFilter::from_default_env();

    let layer = tracing_tree::HierarchicalLayer::default()
        .with_writer(std::io::stdout)
        .with_indent_lines(true)
        .with_indent_amount(2)
        .with_verbose_entry(false)
        .with_verbose_exit(false)
        .with_targets(true);

    let subscriber = Registry::default().with(layer).with(filter);

    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[tokio::main]
async fn main() {
    logger();

    let opts = Options::parse();

    if opts.cleanup_all {
        let mut mem = engine::Memory::new().await.unwrap();
        mem.cleanup(None).await;
        return;
    }

    let mut source = String::with_capacity(20);

    let Some(file) = opts.file else {
        println!("no filename provided, `--help` for details");
        return;
    };

    if let Err(err) =
        std::fs::File::open(&file).and_then(|mut file| file.read_to_string(&mut source))
    {
        eprintln!("Could not read source file `{}: {err}`", file.display());

        std::process::exit(1);
    }

    match parser::Parser::new(&source).parse() {
        Err(err) => {
            err.to_line_error(&source).emit();
            std::process::exit(1)
        }
        Ok(ast) if opts.print_ast => {
            println!("{ast:#?}");
        }
        Ok(ast) => {
            type_check::TypeChecker::new(&ast).everything();

            let engine = engine::TreeWalker::new(&ast).await.unwrap();

            let mut mem = engine.run().await;

            if opts.cleanup {
                mem.cleanup_session().await;
            }
        }
    }
}
