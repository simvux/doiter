use super::lexer::Token;
use logos::Span;
use std::collections::HashMap;

pub struct Parser<'src> {
    lexer: std::iter::Peekable<logos::SpannedIter<'src, Token>>,
}

#[derive(Debug)]
pub struct AST {
    pub types: HashMap<String, Structure>,
    pub functions: HashMap<String, Function>,
    pub statements: Vec<Statement>,
}

#[derive(Debug)]
pub struct Function {
    pub arguments: Vec<(String, Type)>,
    pub ret: Type,
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub struct Structure {
    pub fields: Vec<(String, Type)>,
}

#[derive(Debug)]
pub enum Statement {
    Call { func: String, parameters: Vec<Expr> },
    Var { name: String, ty: Type, rhs: Expr },
    When { cond: Expr, then: Box<Statement> },
    Return(Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Byte(u8),
    Char(char),
    Null,
    Binop(String, Box<[Self; 2]>),
    Call {
        func: String,
        parameters: Vec<Expr>,
    },
    FieldOf(String, String),
    Constructor {
        name: String,
        fields: Vec<(String, Expr)>,
    },
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Type {
    Byte,
    Char,
    Structure(String),
}

#[derive(Debug)]
pub enum Error {
    InvalidToken(Span),
    Unexpected(Token, Span),
    UnexpectedEOF(String),
    MissingType(String, Span),
    MissingReturn(Span),
    DoubleKey(Span),
}

impl<'src> Parser<'src> {
    pub fn new(src: &'src str) -> Self {
        Self {
            lexer: logos::Lexer::new(src).spanned().peekable(),
        }
    }

    // get a mandatory next token
    fn nextm<F>(&mut self, expected: F) -> Result<(Token, Span), Error>
    where
        F: FnOnce() -> String,
    {
        self.lexer
            .next()
            .ok_or_else(|| Error::UnexpectedEOF(expected()))
            .and_then(|(token, span)| match token {
                Ok(token) => Ok((token, span)),
                Err(()) => Err(Error::InvalidToken(span)),
            })
    }

    fn expect(&mut self, token: Token) -> Result<(), Error> {
        self.nextm(|| token.describe()).map(|_| ())
    }

    // Consumes the next token only if it matches predicate `f`
    fn next_as(&mut self, f: impl FnOnce(&Token) -> bool) -> Result<Option<Token>, Error> {
        match self.lexer.peek() {
            Some((Ok(t), _)) if f(t) => {
                let token = self.lexer.next().unwrap().0.unwrap();
                Ok(Some(token))
            }
            None => Err(Error::UnexpectedEOF(Token::SemiColon.describe())),
            _ => Ok(None),
        }
    }

    fn name(&mut self) -> Result<(String, Span), Error> {
        self.nextm(|| "identifier".into())
            .and_then(|(token, span)| match token {
                Token::Name(name) => Ok((name, span)),
                _ => Err(Error::Unexpected(token, span)),
            })
    }

    pub fn parse(&mut self) -> Result<AST, Error> {
        let _handle = tracing::info_span!("Parsing source file");
        let _handle = _handle.enter();

        let mut ast = AST {
            types: HashMap::new(),
            statements: vec![],
            functions: HashMap::new(),
        };

        while let Some((token, span)) = self.lexer.next() {
            let token = token.map_err(|()| Error::InvalidToken(span.clone()))?;

            match token {
                Token::Func => {
                    let (name, _) = self.name()?;
                    let (arguments, ret) = self.fsignature()?;
                    self.expect(Token::CurlyOpen)?;
                    let statements = self.fstatements()?;
                    ast.functions.insert(
                        name,
                        Function {
                            arguments,
                            ret,
                            statements,
                        },
                    );
                }
                Token::Type => {
                    let (name, _) = self.name()?;
                    self.expect(Token::CurlyOpen)?;
                    let fields = self.fields(|this| this.type_())?;
                    ast.types.insert(name, Structure { fields });
                }
                Token::Var => {
                    let stm = self.var()?;
                    ast.statements.push(stm);
                }
                Token::When => {
                    let (cond, then) = self.when()?;
                    ast.statements.push(Statement::When {
                        cond,
                        then: Box::new(then),
                    });
                }
                Token::Name(func) => {
                    let parameters = self.parameters()?;
                    ast.statements.push(Statement::Call { func, parameters });
                }
                Token::SemiColon => continue,

                _ => return Err(Error::Unexpected(token, span)),
            }
        }

        Ok(ast)
    }

    fn var(&mut self) -> Result<Statement, Error> {
        let (name, _) = self.name()?;
        self.expect(Token::Colon)?;
        let ty = self.type_()?;
        self.expect(Token::Operator("=".to_string()))?;
        let rhs = self.expr()?;
        if let Some(_) = self.next_as(|t| *t == Token::SemiColon)? {}
        Ok(Statement::Var { name, ty, rhs })
    }

    fn when(&mut self) -> Result<(Expr, Statement), Error> {
        let cond = self.expr()?;
        self.expect(Token::Colon)?;
        let (token, span) = self.nextm(|| "statement".into())?;
        let then = self.fstatement((token, span))?;
        Ok((cond, then))
    }

    fn fsignature(&mut self) -> Result<(Vec<(String, Type)>, Type), Error> {
        let mut arguments = vec![];

        loop {
            let (token, span) = self.nextm(|| "function argument or `}`".into())?;

            match token {
                Token::CurlyOpen => break Err(Error::MissingReturn(span)),
                Token::Arrow => {
                    let ret = self.type_()?;
                    break Ok((arguments, ret));
                }
                Token::ParenOpen => {
                    let (name, _) = self.name()?;
                    self.expect(Token::Colon)?;
                    let type_ = self.type_()?;
                    self.expect(Token::ParenClose)?;
                    arguments.push((name, type_));
                }
                Token::Name(name) => break Err(Error::MissingType(name, span)),
                _ => break Err(Error::Unexpected(token, span)),
            }
        }
    }

    fn fstatements(&mut self) -> Result<Vec<Statement>, Error> {
        let mut statements = vec![];

        loop {
            let (token, span) = self.nextm(|| "statement or `}`".into())?;

            if token == Token::CurlyClose {
                break Ok(statements);
            }

            let stm = self.fstatement((token, span))?;
            statements.push(stm);
        }
    }

    fn fstatement(
        &mut self,
        (token, span): (Token, std::ops::Range<usize>),
    ) -> Result<Statement, Error> {
        match token {
            Token::Var => self.var(),
            Token::Return => {
                let value = self.expr()?;
                if let Some(_) = self.next_as(|t| *t == Token::SemiColon)? {}
                Ok(Statement::Return(value))
            }
            Token::When => {
                let (cond, then) = self.when()?;
                Ok(Statement::When {
                    cond,
                    then: Box::new(then),
                })
            }
            Token::Name(func) => {
                let parameters = self.parameters()?;
                Ok(Statement::Call { func, parameters })
            }
            _ => Err(Error::Unexpected(token, span)),
        }
    }

    pub fn type_(&mut self) -> Result<Type, Error> {
        let (token, span) = self.nextm(|| "type".into())?;

        match token {
            Token::Name(name) => match name.as_str() {
                "byte" => Ok(Type::Byte),
                "char" => Ok(Type::Char),
                _ => Ok(Type::Structure(name)),
            },
            _ => Err(Error::Unexpected(token, span)),
        }
    }

    fn fields<T, F>(&mut self, mut f: F) -> Result<Vec<(String, T)>, Error>
    where
        F: FnMut(&mut Self) -> Result<T, Error>,
    {
        let mut fields = Vec::new();

        if let Some(_) = self.next_as(|t| *t == Token::CurlyClose)? {
            return Ok(fields);
        }

        loop {
            let (k, kspan) = self.name()?;
            self.expect(Token::Colon)?;
            let v = f(self)?;

            if fields.iter().any(|(previous_key, _)| k == *previous_key) {
                return Err(Error::DoubleKey(kspan));
            }
            fields.push((k, v));

            let (token, span) = self.nextm(|| "`,` or `}`".into())?;
            match token {
                Token::Comma => {
                    // Skip arbritary amount of commas
                    while let Ok(Some(_)) = self.next_as(|token| matches!(token, Token::Comma)) {}

                    // Allow trailing comma
                    if let Some(_) = self.next_as(|t| matches!(t, Token::CurlyClose))? {
                        return Ok(fields);
                    }

                    continue;
                }
                Token::CurlyClose => {
                    return Ok(fields);
                }
                _ => return Err(Error::Unexpected(token, span)),
            }
        }
    }

    fn parameters(&mut self) -> Result<Vec<Expr>, Error> {
        let mut parameters = vec![];

        while let Some(expr) = self.parameter()? {
            parameters.push(expr);
        }

        Ok(parameters)
    }

    fn parameter(&mut self) -> Result<Option<Expr>, Error> {
        match self.lexer.peek() {
            None => return Ok(None),
            Some((result, span)) => match result
                .clone()
                .map_err(|_| Error::InvalidToken(span.clone()))?
            {
                Token::Name(func) => {
                    self.lexer.next();

                    Ok(Some(Expr::Call {
                        func,
                        parameters: vec![],
                    }))
                }
                Token::Null => {
                    self.lexer.next();
                    Ok(Some(Expr::Null))
                }
                Token::Int(n) => {
                    self.lexer.next();
                    Ok(Some(Expr::Byte(n as u8)))
                }
                Token::FieldOf((lhs, rhs)) => {
                    self.lexer.next();
                    Ok(Some(Expr::FieldOf(lhs, rhs)))
                }
                Token::ParenOpen => {
                    self.lexer.next();
                    self.parens(|this| this.expr()).map(Some)
                }
                Token::CharLiteral(char) => {
                    self.lexer.next();
                    Ok(Some(Expr::Char(char)))
                }
                Token::SemiColon => {
                    self.lexer.next();
                    Ok(None)
                }

                _ => Ok(None),
            },
        }
    }

    fn parens<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        let inner = f(self)?;
        self.expect(Token::ParenClose)?;
        Ok(inner)
    }

    fn expr(&mut self) -> Result<Expr, Error> {
        let (token, span) = self.nextm(|| "expression".into())?;

        let mut expr = match token {
            Token::Null => Expr::Null,
            Token::Name(func) => {
                if let Some(_) = self.next_as(|token| matches!(token, Token::CurlyOpen))? {
                    let fields = self.fields(|this| this.expr())?;
                    Expr::Constructor { name: func, fields }
                } else {
                    let parameters = self.parameters()?;
                    Expr::Call { func, parameters }
                }
            }
            Token::Int(n) => Expr::Byte(n as u8),
            Token::FieldOf((lhs, rhs)) => Expr::FieldOf(lhs, rhs),
            Token::ParenOpen => self.parens(|this| this.expr())?,
            Token::CharLiteral(char) => Expr::Char(char),

            Token::Comment => return self.expr(),

            _ => return Err(Error::Unexpected(token, span)),
        };

        while let Some(Token::Operator(op)) = self.next_as(|t| matches!(t, Token::Operator(_)))? {
            let Some(rhs) = self.parameter()? else {
                break;
            };

            expr = Expr::Binop(op, Box::new([expr, rhs]));
        }

        Ok(expr)
    }
}

impl Error {
    pub fn to_line_error(&self, src: &str) -> super::error::Error {
        match self {
            Error::InvalidToken(span) => {
                super::error::Error::new("invalid token").at(src, span.clone())
            }
            Error::Unexpected(token, span) => {
                super::error::Error::new(format!("unexpected token: {}", token.describe()))
                    .at(src, span.clone())
            }
            Error::UnexpectedEOF(expected) => {
                super::error::Error::new(format!("unexpected end of file, expected: {expected}"))
                    .at(src, src.len() - 1..src.len())
            }
            Error::MissingType(name, span) => {
                super::error::Error::new(format!("`{name}` is missing its type"))
                    .at(src, span.clone())
            }
            Error::MissingReturn(span) => {
                super::error::Error::new(format!("function is missing its return type"))
                    .at(src, span.clone())
            }
            Error::DoubleKey(span) => {
                super::error::Error::new(format!("field key is already assigned"))
                    .at(src, span.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(name: &'static str, src: &str) {
        let mut parser = Parser::new(src);
        let str = match parser.parse() {
            Ok(ast) => format!("{ast:#?}"),
            Err(error) => error.to_line_error(src).to_string(),
        };

        insta::assert_snapshot!(name, str);
    }

    #[test]
    fn type_point() {
        let src = "
            type Point {
                x: int,
                y: int
            }
        ";

        snapshot("type_point", src);
    }

    #[test]
    fn var_char() {
        let src = "
            var a: char = 'a';
            var b: char = 'b';
        ";

        snapshot("var_char", src);
    }

    #[test]
    fn len_signature() {
        let src = "
            func get (list : ByteList) (elem : byte) -> byte {
                
            }
        ";

        snapshot("len_signature", src);
    }

    #[test]
    fn print_point_x() {
        let src = "
            print point.x;
        ";

        snapshot("print_point_x", src);
    }

    #[test]
    fn operators() {
        let src = "
            var byte: b = x == null;
        ";

        snapshot("operators", src);
    }
}
