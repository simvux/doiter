use super::parser as ast;
use super::parser::AST;
use logos::Span;
use std::collections::HashMap;

pub struct TypeChecker<'a, 's> {
    ast: &'a AST,

    src: &'s str,
    span: Span,
    failed: bool,
    bindings: HashMap<&'a str, ast::Type>,
}

impl<'a, 's> TypeChecker<'a, 's> {
    pub fn new(ast: &'a AST, src: &'s str) -> Self {
        Self {
            ast,
            span: 0..1,
            failed: false,
            bindings: HashMap::new(),
            src,
        }
    }

    pub fn everything(&mut self) -> bool {
        for (_, func) in self.ast.functions.iter() {
            self.span = func.span.clone();
            self.bindings.clear();

            for (name, ty) in &func.arguments {
                self.bindings.insert(name, ty.clone());
            }

            if let Some(ty) = self.statements(&func.statements) {
                if func.ret != ty {
                    panic!("ET: type mismatch");
                }
            } else {
                panic!("ET: missing return type");
            };
        }

        self.bindings.clear();
        if let Some(_) = self.statements(&self.ast.statements) {
            panic!("ET: cannot return from top-level");
        }

        self.failed
    }

    fn add_err(&mut self, msg: impl Into<String>) {
        self.failed = true;

        crate::error::Error::new(msg)
            .at(self.src, self.span.clone())
            .emit();
    }

    fn statements(&mut self, statements: &'a [(ast::Statement, Span)]) -> Option<ast::Type> {
        let mut ret = None;

        for (statement, span) in statements {
            self.span = span.clone();

            match statement {
                ast::Statement::Call { func, parameters } => {
                    self.func_call(func, parameters);
                }
                ast::Statement::Var { name, ty, rhs } => {
                    self.expr_is_type_of(rhs, ty);
                    self.bindings.insert(name.as_str(), ty.clone());
                }
                ast::Statement::When { cond, .. } => self.expr_is_type_of(cond, &ast::Type::Byte),
                ast::Statement::Return(expr) => match &ret {
                    Some(previous) => self.expr_is_type_of(expr, &previous),
                    None if *expr == ast::Expr::Null => {}
                    None => ret = Some(self.type_of(expr)),
                },
            }
        }

        ret
    }

    fn expr_is_type_of(&mut self, expr: &'a ast::Expr, ty: &ast::Type) {
        if *expr == ast::Expr::Null {
            if let ast::Type::Structure(_) = ty {
                return;
            }
        }

        let got = self.type_of(expr);
        if got != *ty {
            self.add_err(format!("type mismatch: {got} != {ty}"));
        }
    }

    fn type_of(&mut self, expr: &'a ast::Expr) -> ast::Type {
        match expr {
            ast::Expr::Null => unreachable!(),
            ast::Expr::Byte(_) => ast::Type::Byte,
            ast::Expr::Char(_) => ast::Type::Char,
            ast::Expr::Binop(op, sides) => match op.as_str() {
                "==" | "+" | "-" | "/" | "*" | "%" | "<" | ">" | ">=" | "<=" => {
                    if sides[0] == ast::Expr::Null || sides[1] == ast::Expr::Null {
                        return ast::Type::Byte;
                    }

                    let lhs = self.type_of(&sides[0]);
                    let rhs = self.type_of(&sides[1]);

                    if lhs != rhs {
                        self.add_err(format!("type mismatch: {lhs} != {rhs}"));
                    }

                    ast::Type::Byte
                }
                _ => {
                    self.add_err(format!("unknown operator: {op}"));
                    ast::Type::Byte
                }
            },
            ast::Expr::Call { func, parameters } => self.func_call(func, parameters),
            ast::Expr::FieldOf(lhs, rhs) => {
                let Some(lhs) = self.bindings.get(&**lhs) else {
                    self.add_err(format!("unknown variable: {lhs}"));
                    return ast::Type::Byte;
                };

                let ast::Type::Structure(type_name) = lhs else {
                    self.add_err(format!("cannot get field {rhs} of type {lhs}"));
                    return ast::Type::Byte;
                };

                let Some(structure) = self.ast.types.get(type_name) else {
                    self.add_err(format!("structure type {type_name} not found"));
                    return ast::Type::Byte;
                };

                let Some((_, ty)) = structure.fields.iter().find(|(name, _)| name == rhs) else {
                    self.add_err(format!(
                        "structure {type_name} does not have a field named {rhs}"
                    ));
                    return ast::Type::Byte;
                };

                ty.clone()
            }
            ast::Expr::Constructor { name, fields } => {
                let Some(structure) = self.ast.types.get(name) else {
                    self.add_err(format!("structure type {name} not found"));
                    return ast::Type::Byte;
                };

                for (name, type_) in &structure.fields {
                    let Some((_, expr)) = fields.iter().find(|(n, _)| *name == *n) else {
                        self.add_err(format!("missing field {name}"));

                        continue;
                    };

                    self.expr_is_type_of(expr, type_)
                }

                for (name, _) in fields {
                    if structure.fields.iter().all(|(n, _)| n != name) {
                        self.add_err(format!(
                            "structure {name} does not have a field named {name}"
                        ));
                    }
                }

                ast::Type::Structure(name.clone())
            }
        }
    }

    fn func_call(&mut self, func: &'a str, parameters: &'a [ast::Expr]) -> ast::Type {
        if parameters.is_empty() {
            if let Some(ty) = self.bindings.get(func) {
                return ty.clone();
            }
        }

        match func {
            "exit" => return ast::Type::Byte,
            "print" => return ast::Type::Byte,
            _ => {
                let Some(func) = self.ast.functions.get(func) else {
                    todo!("ET: not found: {func}");
                };

                if parameters.len() != func.arguments.len() {
                    panic!("ET: parameter mismatch");
                }

                for (expr, (_, expected)) in parameters.iter().zip(&func.arguments) {
                    self.expr_is_type_of(expr, expected)
                }

                func.ret.clone()
            }
        }
    }
}
