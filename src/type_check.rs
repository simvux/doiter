use super::parser as ast;
use super::parser::AST;
use std::collections::HashMap;

pub struct TypeChecker<'a> {
    ast: &'a AST,

    bindings: HashMap<&'a str, ast::Type>,
}

impl<'a> TypeChecker<'a> {
    pub fn new(ast: &'a AST) -> Self {
        Self {
            ast,
            bindings: HashMap::new(),
        }
    }

    pub fn everything(&mut self) {
        for (_, func) in self.ast.functions.iter() {
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
    }

    fn statements(&mut self, statements: &'a [ast::Statement]) -> Option<ast::Type> {
        let mut ret = None;

        for statement in statements {
            match statement {
                ast::Statement::Call { func, parameters } => {
                    self.func_call(func, parameters);
                }
                ast::Statement::Var { name, ty, rhs } => {
                    if !self.expr_is_type_of(rhs, ty) {
                        panic!("ET: type mismatch: {rhs:?} != {ty:?}");
                    }

                    self.bindings.insert(name.as_str(), ty.clone());
                }
                ast::Statement::When { cond, .. } => {
                    if !self.expr_is_type_of(cond, &ast::Type::Byte) {
                        panic!("ET: type mismatch");
                    }
                }
                ast::Statement::Return(expr) => match &ret {
                    Some(previous) => {
                        if !self.expr_is_type_of(expr, &previous) {
                            panic!("ET: type mismatch");
                        }
                    }
                    None if *expr == ast::Expr::Null => {}
                    None => ret = Some(self.type_of(expr)),
                },
            }
        }

        ret
    }

    fn expr_is_type_of(&mut self, expr: &'a ast::Expr, ty: &ast::Type) -> bool {
        if *expr == ast::Expr::Null {
            if let ast::Type::Structure(_) = ty {
                return true;
            }
        }

        self.type_of(expr) == *ty
    }

    fn type_of(&mut self, expr: &'a ast::Expr) -> ast::Type {
        match expr {
            ast::Expr::Null => unreachable!(),
            ast::Expr::Byte(_) => ast::Type::Byte,
            ast::Expr::Char(_) => ast::Type::Char,
            ast::Expr::Binop(op, sides) => match op.as_str() {
                "==" => {
                    if sides[0] == ast::Expr::Null || sides[1] == ast::Expr::Null {
                        return ast::Type::Byte;
                    }

                    let lhs = self.type_of(&sides[0]);
                    let rhs = self.type_of(&sides[1]);

                    if lhs != rhs {
                        panic!("ET: type mismatch");
                    }

                    ast::Type::Byte
                }
                _ => panic!("ET: unknown operator"),
            },
            ast::Expr::Call { func, parameters } => self.func_call(func, parameters),
            ast::Expr::FieldOf(lhs, rhs) => {
                let Some(lhs) = self.bindings.get(&**lhs) else {
                    // TODO: rename typecheck to checker
                    panic!("ET: variable not found: {lhs}");
                };

                let ast::Type::Structure(type_name) = lhs else {
                    panic!("ET: field of non-struct");
                };

                let Some(structure) = self.ast.types.get(type_name) else {
                    panic!("ET: structure not found");
                };

                let Some((_, ty)) = structure.fields.iter().find(|(name, _)| name == rhs) else {
                    panic!("ET: field not found: {rhs}");
                };

                ty.clone()
            }
            ast::Expr::Constructor { name, fields } => {
                let Some(structure) = self.ast.types.get(name) else {
                    panic!("ET: structure not found");
                };

                for (name, type_) in &structure.fields {
                    let Some((_, expr)) = fields.iter().find(|(n, _)| *name == *n) else {
                        panic!("ET: missing field");
                    };

                    if !self.expr_is_type_of(expr, type_) {
                        panic!("ET: typs mismatch");
                    }
                }

                for (name, _) in fields {
                    if structure.fields.iter().all(|(n, _)| n != name) {
                        panic!("ET: field not found: {name}");
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
            "print" => return ast::Type::Byte,
            _ => {
                let Some(func) = self.ast.functions.get(func) else {
                    todo!("ET: not found: {func}");
                };

                if parameters.len() != func.arguments.len() {
                    panic!("ET: parameter mismatch");
                }

                for (expr, (_, expected)) in parameters.iter().zip(&func.arguments) {
                    if !self.expr_is_type_of(expr, expected) {
                        todo!("ET: type mismatch: {expr:?} != {expected:?}");
                    }
                }

                func.ret.clone()
            }
        }
    }
}
