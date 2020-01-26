
use combine::*;
use combine::parser::char::*;
use combine::parser::range::*;
use combine::error::*;

use std::fmt::{self, Display};

// TODO: Fix error handling

#[derive(Debug, PartialEq)]
pub struct Expr<'a> {
    pub inner: ExprInner<'a>,
    pub ty: Option<Type>,
}

/// A type used by the rule system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Type {
    /// Boolean - true or false
    Boolean,
    /// Integer - 32 bit signed integer
    Integer,
    /// Float   - 32 bit float
    Float,
}

impl <'a> From<ExprInner<'a>> for Expr<'a> {
    fn from(v: ExprInner<'a>) -> Expr<'a> {
        Expr {
            inner: v,
            ty: None,
        }
    }
}
impl <'a> Display for Expr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}


#[derive(Debug, PartialEq)]
pub enum ExprInner<'a> {
    Get(&'a str),
    GetGlobal(&'a str),

    Boolean(bool),
    Float(f32),
    Integer(i32),

    Not(Box<Expr<'a>>),
    And(Box<Expr<'a>>, Box<Expr<'a>>),
    Or(Box<Expr<'a>>, Box<Expr<'a>>),
    Xor(Box<Expr<'a>>, Box<Expr<'a>>),

    Add(Box<Expr<'a>>, Box<Expr<'a>>),
    Sub(Box<Expr<'a>>, Box<Expr<'a>>),
    Mul(Box<Expr<'a>>, Box<Expr<'a>>),
    Div(Box<Expr<'a>>, Box<Expr<'a>>),
    Rem(Box<Expr<'a>>, Box<Expr<'a>>),

    Equal(Box<Expr<'a>>, Box<Expr<'a>>),
    NotEqual(Box<Expr<'a>>, Box<Expr<'a>>),
    LessEqual(Box<Expr<'a>>, Box<Expr<'a>>),
    GreaterEqual(Box<Expr<'a>>, Box<Expr<'a>>),
    Less(Box<Expr<'a>>, Box<Expr<'a>>),
    Greater(Box<Expr<'a>>, Box<Expr<'a>>),

    IntToFloat(Box<Expr<'a>>),
    FloatToInt(Box<Expr<'a>>),
}

impl <'a> Display for ExprInner<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprInner::Get(name) => write!(f, "{}", name),
            ExprInner::GetGlobal(name) => write!(f, "global.{}", name),
            ExprInner::Boolean(b) => write!(f, "{}", b),
            ExprInner::Float(fl) => write!(f, "{}f", fl),
            ExprInner::Integer(i) => write!(f, "{}", i),

            ExprInner::Not(e) => write!(f, "!{}", e),
            ExprInner::And(l, r) => write!(f, "({} && {})", l, r),
            ExprInner::Or(l, r) => write!(f, "({} || {})", l, r),
            ExprInner::Xor(l, r) => write!(f, "({} ^ {})", l, r),

            ExprInner::Add(l, r) => write!(f, "({} + {})", l, r),
            ExprInner::Sub(l, r) => write!(f, "({} - {})", l, r),
            ExprInner::Mul(l, r) => write!(f, "({} * {})", l, r),
            ExprInner::Div(l, r) => write!(f, "({} / {})", l, r),
            ExprInner::Rem(l, r) => write!(f, "({} % {})", l, r),

            ExprInner::Equal(l, r) => write!(f, "({} == {})", l, r),
            ExprInner::NotEqual(l, r) => write!(f, "({} != {})", l, r),
            ExprInner::LessEqual(l, r) => write!(f, "({} <= {})", l, r),
            ExprInner::GreaterEqual(l, r) => write!(f, "({} >= {})", l, r),
            ExprInner::Less(l, r) => write!(f, "({} < {})", l, r),
            ExprInner::Greater(l, r) => write!(f, "({} > {})", l, r),

            ExprInner::IntToFloat(e) => write!(f, "float({})", e),
            ExprInner::FloatToInt(e) => write!(f, "int({})", e),
        }
    }
}

fn expr<'a>(input: &mut &'a str) -> ParseResult<Expr<'a>, &'a str>
{
    let skip_spaces = || spaces().silent();

    let (mut current, _) =
        skip_spaces()
        .with(parser(bool_ops))
        .skip(skip_spaces())
        .parse_stream(input)?;

    while let Ok((op, _)) = choice((
                attempt(string("==")),
                attempt(string("!=")),
                attempt(string("<=")),
                attempt(string(">=")),
                string("<"),
                string(">"),
            ))
            .skip(skip_spaces())
            .parse_stream(input)
    {
        let (other, _) = parser(bool_ops)
            .skip(skip_spaces())
            .parse_stream(input)?;
        current = match op {
            "==" => ExprInner::Equal(Box::new(current), Box::new(other)),
            "!=" => ExprInner::NotEqual(Box::new(current), Box::new(other)),
            "<=" => ExprInner::LessEqual(Box::new(current), Box::new(other)),
            ">=" => ExprInner::GreaterEqual(Box::new(current), Box::new(other)),
            "<" => ExprInner::Less(Box::new(current), Box::new(other)),
            ">" => ExprInner::Greater(Box::new(current), Box::new(other)),
            _ => unreachable!(),
        }.into();
    }
    Ok((current, Consumed::Consumed(())))
}

fn bool_ops<'a>(input: &mut &'a str) -> ParseResult<Expr<'a>, &'a str>
{
    let skip_spaces = || spaces().silent();

    let (mut current, _) = parser(term1)
        .skip(skip_spaces())
        .parse_stream(input)?;

    while let Ok((op, _)) = choice((
                attempt(string("&&")),
                attempt(string("||")),
                string("^"),
            ))
            .skip(skip_spaces())
            .parse_stream(input)
    {
        let (other, _) = parser(term1)
            .skip(skip_spaces())
            .parse_stream(input)?;
        current = match op {
            "&&" => ExprInner::And(Box::new(current), Box::new(other)),
            "||" => ExprInner::Or(Box::new(current), Box::new(other)),
            "^" => ExprInner::Xor(Box::new(current), Box::new(other)),
            _ => unreachable!(),
        }.into();
    }

    Ok((current, Consumed::Consumed(())))
}

fn term1<'a>(input: &mut &'a str) -> ParseResult<Expr<'a>, &'a str>
{
    let skip_spaces = || spaces().silent();

    let (mut current, _) = parser(term2)
        .skip(skip_spaces())
        .parse_stream(input)?;

    while let Ok((op, _)) = choice((char('+'), char('-')))
            .skip(skip_spaces())
            .parse_stream(input)
    {
        let (other, _) = parser(term2)
            .skip(skip_spaces())
            .parse_stream(input)?;
        current = match op {
            '+' => ExprInner::Add(Box::new(current), Box::new(other)),
            '-' => ExprInner::Sub(Box::new(current), Box::new(other)),
            _ => unreachable!(),
        }.into();
    }

    Ok((current, Consumed::Consumed(())))
}

fn term2<'a>(input: &mut &'a str) -> ParseResult<Expr<'a>, &'a str>
{
    let skip_spaces = || spaces().silent();

    let (mut current, _) = factor()
        .skip(skip_spaces())
        .parse_stream(input)?;

    while let Ok((op, _)) = choice((char('*'), char('/'), char('%')))
            .skip(skip_spaces())
            .parse_stream(input)
    {
        let (other, _) = factor()
            .skip(skip_spaces())
            .parse_stream(input)?;
        current = match op {
            '*' => ExprInner::Mul(Box::new(current), Box::new(other)),
            '/' => ExprInner::Div(Box::new(current), Box::new(other)),
            '%' => ExprInner::Rem(Box::new(current), Box::new(other)),
            _ => unreachable!(),
        }.into();
    }
    Ok((current, Consumed::Consumed(())))
}

fn factor<'a>() -> impl Parser<Input = &'a str, Output = Expr<'a>>
{
    let skip_spaces = || spaces().silent();

    let global = string("global")
        .skip(skip_spaces())
        .with(char('.'))
        .skip(skip_spaces())
        .with(look_ahead(letter()))
        .with(take_while1(|c: char| c.is_alphanumeric() || c == '_'))
        .map(ExprInner::GetGlobal)
        .map(Into::into);

    let field = look_ahead(letter())
        .with(take_while1(|c: char| c.is_alphanumeric() || c == '_'))
        .map(ExprInner::Get)
        .map(Into::into);

    let brackets = char('(')
        .skip(skip_spaces())
        .with(parser(expr))
        .skip(skip_spaces())
        .skip(char(')'));

    let float_to_int = string("int(")
        .skip(skip_spaces())
        .with(parser(expr))
        .map(|v| ExprInner::FloatToInt(Box::new(v)))
        .map(Into::into)
        .skip(skip_spaces())
        .skip(char(')'));
    let int_to_float = string("float(")
        .skip(skip_spaces())
        .with(parser(expr))
        .map(|v| ExprInner::IntToFloat(Box::new(v)))
        .map(Into::into)
        .skip(skip_spaces())
        .skip(char(')'));

    let not = char('!')
        .skip(skip_spaces())
        .with(parser(expr))
        .map(|v| ExprInner::Not(Box::new(v)))
        .map(Into::into);

    let t = string("true")
        .map(|_| ExprInner::Boolean(true))
        .map(Into::into);
    let f = string("false")
        .map(|_| ExprInner::Boolean(false))
        .map(Into::into);

    let float = from_str(recognize(
            optional(char('-'))
                .with(
                    take_while1(|c: char| c.is_digit(10) || c == '.')
                        .and_then(|v: &str| if v.contains('.') { Ok(v) } else { Err(StringStreamError::UnexpectedParse)} )
                )
        ))
        .map(ExprInner::Float)
        .map(Into::into);

    let int = from_str(recognize(
            optional(char('-'))
                .with(take_while1(|c: char| c.is_digit(10)))
        ))
        .map(ExprInner::Integer)
        .map(Into::into);


    choice((
        attempt(float_to_int),
        attempt(int_to_float),
        attempt(t),
        attempt(f),
        attempt(global),
        attempt(field),
        attempt(float),
        attempt(int),
        attempt(brackets),
        attempt(not),
    ))
}

pub fn parse(val: &str) -> crate::errors::Result<Expr<'_>> {
    match parser(expr).parse(val) {
        Ok((expr, rem)) => if rem.trim().is_empty() {
            Ok(expr)
        } else {
            Err(crate::errors::ErrorKind::IncompleteParse(rem.into()).into())
        },
        Err(_err) => Err(crate::errors::ErrorKind::ParseFailed(val.into()).into()),
    }
}

#[test]
fn test() {
    macro_rules! test_parse {
        ($test:expr) => {
            match parser(expr).parse($test) {
                Ok((_, remaining)) => {
                    if !remaining.trim().is_empty() {
                        panic!("Test {:?} had {:?} remaining", $test, remaining);
                    }
                }
                Err(err) => panic!("Failed parsing {:?} with error: {:?}", $test, err),
            }
        };
    }

    test_parse!("         global    .   hello");
    test_parse!("   test_field");
    test_parse!("a+b");
    test_parse!("a+b-c");
    test_parse!("a*b/c");
    test_parse!("a+b*c");
    test_parse!("(a+b)*c");
    test_parse!("!a");
    test_parse!("(a+!b)*c");
    test_parse!("true");
    test_parse!("false");

    test_parse!("5.32");
    test_parse!("67");
    test_parse!("2+34*65");
    test_parse!("54.23 + 67.2 * 0.93421");

    test_parse!("a == b");
    test_parse!("a != b");
    test_parse!("a <= b");
    test_parse!("a >= b");
    test_parse!("a < b");
    test_parse!("a > b");

    test_parse!("a && b");
    test_parse!("a || b");
    test_parse!("a ^ b");

    test_parse!("(a * 5 + 3) == (b + 2 * 3)");
    test_parse!("(a*5+3)==(b+2*3)");
    test_parse!("float(a*5+3)");
    test_parse!("int(a*5.2+3.1)");
}