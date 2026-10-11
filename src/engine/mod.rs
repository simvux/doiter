use logos::Span;
use std::path::PathBuf;

use crate::parser as ast;
use crate::parser::AST;
use async_recursion::async_recursion;
use std::collections::HashMap;

mod docker;
pub use docker::{Memory, RootAddress};

pub struct TreeWalker<'a> {
    ast: &'a AST,
    mem: Memory,

    frames: Vec<StackFrame>,
}

#[derive(Debug)]
struct StackFrame {
    types: HashMap<String, ast::Type>,
    id: RootAddress,
}

#[derive(Debug, PartialEq, Eq)]
enum Value {
    Null,
    Byte(u8),
    Char(char),
    Structure(RootAddress),
}

enum ControlFlow {
    Exit,
    Return(Value),
    Continue,
}

impl<'a> TreeWalker<'a> {
    pub async fn new(ast: &'a AST) -> Result<Self, bollard::errors::Error> {
        let heap = Memory::new().await?;

        Ok(Self {
            ast,
            mem: heap,
            frames: vec![],
        })
    }

    pub async fn run(mut self) -> Memory {
        let _handle = tracing::info_span!("Interpreting top-level statements");
        let _handle = _handle.enter();

        let main = self.mem.alloc().await;
        self.enter_frame(main).await;
        self.statements(&self.ast.statements).await;

        self.mem
    }

    async fn enter_frame(&mut self, id: RootAddress) {
        self.frames.push(StackFrame {
            id,
            types: HashMap::new(),
        });
    }

    async fn statements(&mut self, statements: &[(ast::Statement, Span)]) -> ControlFlow {
        for (stm, _) in statements {
            let flow = self.statement(stm).await;

            match flow {
                ControlFlow::Exit => return ControlFlow::Exit,
                ControlFlow::Return(value) => return ControlFlow::Return(value),
                ControlFlow::Continue => continue,
            }
        }

        ControlFlow::Continue
    }

    #[async_recursion]
    async fn statement(&mut self, stm: &ast::Statement) -> ControlFlow {
        tracing::info!("statement: {stm:?}");

        match stm {
            ast::Statement::Call { func, parameters } if func == "exit" => ControlFlow::Exit,
            ast::Statement::Call { func, parameters } => {
                if parameters.is_empty() {
                    if let Some(_) = self.resolve_local(func).await {
                        return ControlFlow::Continue;
                    }
                }

                self.call_func(func, parameters).await;
                ControlFlow::Continue
            }
            ast::Statement::When { cond, then } => {
                let cond = self.expr(cond).await;

                let mut flow = ControlFlow::Continue;
                if let Value::Byte(1) = cond {
                    flow = self.statement(&then).await;
                }

                flow
            }
            ast::Statement::Var { name, ty, rhs } => {
                let rhs = self.expr(rhs).await;

                let frame = self.frames.last_mut().unwrap();

                frame.types.insert(name.clone(), ty.clone());

                let addr = frame.id.clone();
                let path = PathBuf::from(format!("/{name}"));
                let mut data = vec![];
                self.encode_value(&rhs, &mut data);
                self.mem.write(&addr, &path, &data).await;

                ControlFlow::Continue
            }
            ast::Statement::Return(expr) => {
                let v = self.expr(expr).await;
                ControlFlow::Return(v)
            }
        }
    }

    async fn call_func(&mut self, func: &str, parameters: &[ast::Expr]) -> Value {
        match func {
            "print" => {
                assert_eq!(parameters.len(), 1);
                let value = self.expr(&parameters[0]).await;

                print!("{}", self.value_to_string(value));
                return Value::Null;
            }
            _ => {
                if parameters.is_empty() {
                    if let Some((value, _)) = self.resolve_local(func).await {
                        return value;
                    }
                }

                let func = self.ast.functions.get(func).unwrap();

                let frame = self.mem.alloc().await;

                for (param, (name, _type)) in parameters.iter().zip(&func.arguments) {
                    let value = self.expr(param).await;
                    self.write(&frame, name, &value).await;
                }

                self.enter_frame(frame).await;

                for (name, ty) in func.arguments.iter() {
                    let frame = self.frames.last_mut().unwrap();
                    frame.types.insert(name.clone(), ty.clone());
                }

                let value = match self.statements(&func.statements).await {
                    ControlFlow::Exit => unreachable!("Cannot exit from an expression"),
                    ControlFlow::Return(value) => value,
                    ControlFlow::Continue => Value::Null,
                };

                self.frames.pop();

                value
            }
        }
    }

    async fn write(&mut self, addr: &RootAddress, name: &str, value: &Value) {
        let path = PathBuf::from(format!("/{name}"));
        let mut data = vec![];
        self.encode_value(&value, &mut data);
        tracing::info!("Writing: {:?} to {addr:#?}::/{name}", &data);
        self.mem.write(&addr, &path, &data).await;
    }

    async fn try_read(&mut self, addr: &RootAddress, name: &str, ty: &ast::Type) -> Option<Value> {
        let path = PathBuf::from(format!("/{name}"));
        let mut bytes = self.mem.try_read(addr, &path).await?;

        // HACK: Some part of the pipeline tries to be clever and converts a file with `10` into carriage return
        if &bytes == b"\r\n" {
            bytes = vec![b'\n'];
        }

        tracing::info!("Reading: {:?} from {addr:#?}::/{name} as {ty:?}", &bytes);

        let (value, xs) = self.decode_value(&bytes, &ty).await;
        assert!(xs.is_empty());

        Some(value)
    }

    async fn read(&mut self, addr: &RootAddress, name: &str, ty: &ast::Type) -> Value {
        self.try_read(addr, name, ty).await.unwrap()
    }

    #[async_recursion]
    async fn expr(&mut self, expr: &ast::Expr) -> Value {
        tracing::info!("expression: {expr:?}");

        match expr {
            ast::Expr::Null => Value::Null,
            ast::Expr::Byte(n) => Value::Byte(*n as u8),
            ast::Expr::Char(c) => Value::Char(*c),
            ast::Expr::Binop(op, sides) => {
                let lhs = self.expr(&sides[0]).await;
                let rhs = self.expr(&sides[1]).await;

                fn on_bytes(nums: [Value; 2], f: fn(u8, u8) -> u8) -> Value {
                    let [lhs, rhs] = nums.map(|n| match n {
                        Value::Byte(n) => n,
                        _ => panic!("not a number: {n:?}"),
                    });

                    Value::Byte(f(lhs, rhs))
                }

                match op.as_str() {
                    "==" => Value::Byte((lhs == rhs) as u8),
                    "+" => on_bytes([lhs, rhs], |x, y| x + y),
                    "-" => on_bytes([lhs, rhs], |x, y| x - y),
                    "*" => on_bytes([lhs, rhs], |x, y| x * y),
                    "/" => on_bytes([lhs, rhs], |x, y| x / y),
                    "<" => on_bytes([lhs, rhs], |x, y| (x < y) as u8),
                    ">" => on_bytes([lhs, rhs], |x, y| (x > y) as u8),
                    ">=" => on_bytes([lhs, rhs], |x, y| (x >= y) as u8),
                    "<=" => on_bytes([lhs, rhs], |x, y| (x <= y) as u8),
                    _ => todo!(),
                }
            }
            ast::Expr::Call { func, parameters } => {
                if parameters.is_empty() {
                    if let Some((value, _)) = self.resolve_local(func).await {
                        return value;
                    }
                }

                self.call_func(func, parameters).await
            }
            ast::Expr::FieldOf(lhs, rhs) => {
                let (lhs, lhs_ty) = self
                    .resolve_local(lhs)
                    .await
                    .unwrap_or_else(|| panic!("not found: {lhs}, cannot get {rhs}"));

                let Value::Structure(address) = &lhs else {
                    panic!("cannot retrieve field of non-structure: {lhs:?}");
                };

                let ast::Type::Structure(structure_name) = lhs_ty else {
                    panic!("cannot retrieve field of non-structure: {lhs:?}");
                };

                let (_, field_type) = self.ast.types[&structure_name]
                    .fields
                    .iter()
                    .find(|(n, _)| *n == *rhs)
                    .expect("cannot retrieve field");

                self.read(&address, rhs, &field_type).await
            }
            ast::Expr::Constructor { name, fields } => {
                let structure = self
                    .ast
                    .types
                    .get(name)
                    .unwrap_or_else(|| panic!("name resolution failure: {name}"));

                let address = self.mem.alloc().await;

                for (fname, _) in structure.fields.iter() {
                    let expr = fields
                        .iter()
                        .find_map(|(n, expr)| (n == fname).then_some(expr))
                        .unwrap_or_else(|| panic!("field missing for {name}: {fname}"));

                    let value = self.expr(&expr).await;
                    self.write(&address, fname, &value).await;
                }

                Value::Structure(address)
            }
        }
    }

    fn value_to_string(&mut self, value: Value) -> String {
        match value {
            Value::Null => "<<null>>".into(),
            Value::Byte(b) => b.to_string(),
            Value::Char(char) => char.to_string(),
            Value::Structure(_) => todo!(),
        }
    }

    async fn resolve_local(&mut self, name: &str) -> Option<(Value, ast::Type)> {
        let frame = self.frames.last_mut().unwrap();

        let ty = frame.types.get(name)?.clone();
        let id = frame.id.clone();

        let value = self.try_read(&id, name, &ty).await.unwrap_or(Value::Null);

        Some((value, ty))
    }

    fn encode_value(&mut self, v: &Value, buf: &mut Vec<u8>) {
        match v {
            Value::Null => {}
            Value::Byte(n) => buf.push(*n),
            Value::Char(c) => {
                c.to_string().bytes().for_each(|b| buf.push(b));
            }
            Value::Structure(addr) => {
                for b in addr.bytes() {
                    buf.push(b);
                }
            }
        }
    }

    #[async_recursion]
    async fn decode_value<'b>(&mut self, bytes: &'b [u8], ty: &ast::Type) -> (Value, &'b [u8]) {
        const CONTAINER_ID_SIZE: usize = 64;

        tracing::info!(
            "reading {ty:?} from {bytes:?}, str: {}",
            String::from_utf8_lossy(bytes)
        );

        match ty {
            ast::Type::Byte => {
                let (x, xs) = bytes.split_first().unwrap();
                (Value::Byte(*x), xs)
            }
            ast::Type::Char => {
                let (x, xs) = bytes.split_first().unwrap();
                (Value::Char(*x as char), xs)
            }
            ast::Type::Structure(_) => {
                tracing::info!("Reading structure from: {}", String::from_utf8_lossy(bytes));

                // FIXME: This will cause undefined behaviour on very large structures
                if bytes.len() < CONTAINER_ID_SIZE {
                    return (Value::Null, bytes);
                }
                let (addr, xs) = bytes.split_at(CONTAINER_ID_SIZE);

                let addr = self
                    .mem
                    .from_raw_bytes(addr)
                    .unwrap_or_else(|| panic!("{addr:#?} is not a valid pointer"));

                (Value::Structure(addr), xs)
            }
        }
    }
}
