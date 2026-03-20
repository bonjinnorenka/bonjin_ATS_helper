use base64::{Engine as _, engine::general_purpose::STANDARD};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    entity::{DynamicEntity, EntityProperty},
    error::ValidationError,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterExpression {
    Comparison {
        left: FilterOperand,
        operator: ComparisonOperator,
        right: FilterOperand,
    },
    And(Box<FilterExpression>, Box<FilterExpression>),
    Or(Box<FilterExpression>, Box<FilterExpression>),
    Not(Box<FilterExpression>),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterOperand {
    Property(String),
    Literal(FilterLiteral),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterLiteral {
    String(String),
    Bool(bool),
    Int(i64),
    Double(f64),
    DateTime(OffsetDateTime),
    Guid(Uuid),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComparisonOperator {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

pub(crate) trait EntityView {
    fn partition_key(&self) -> &str;
    fn row_key(&self) -> &str;
    fn timestamp(&self) -> Option<OffsetDateTime>;
    fn property(&self, name: &str) -> Option<&EntityProperty>;
}

impl EntityView for DynamicEntity {
    fn partition_key(&self) -> &str {
        &self.partition_key
    }

    fn row_key(&self) -> &str {
        &self.row_key
    }

    fn timestamp(&self) -> Option<OffsetDateTime> {
        self.timestamp
    }

    fn property(&self, name: &str) -> Option<&EntityProperty> {
        self.properties.get(name)
    }
}

pub(crate) fn parse_filter(input: &str) -> Result<FilterExpression, ValidationError> {
    let mut parser = Parser::new(input)?;
    let expression = parser.parse_expression()?;
    if !matches!(parser.current.kind, TokenKind::End) {
        return Err(ValidationError::InvalidQuery(
            "unsupported filter syntax".to_owned(),
        ));
    }
    Ok(expression)
}

pub(crate) fn count_comparisons(expression: &FilterExpression) -> usize {
    match expression {
        FilterExpression::Comparison { .. } => 1,
        FilterExpression::And(left, right) | FilterExpression::Or(left, right) => {
            count_comparisons(left) + count_comparisons(right)
        }
        FilterExpression::Not(expression) => count_comparisons(expression),
    }
}

pub(crate) fn evaluate_filter(
    expression: &FilterExpression,
    entity: &impl EntityView,
) -> Result<bool, ValidationError> {
    match expression {
        FilterExpression::Comparison {
            left,
            operator,
            right,
        } => evaluate_comparison(left, *operator, right, entity),
        FilterExpression::And(left, right) => {
            Ok(evaluate_filter(left, entity)? && evaluate_filter(right, entity)?)
        }
        FilterExpression::Or(left, right) => {
            Ok(evaluate_filter(left, entity)? || evaluate_filter(right, entity)?)
        }
        FilterExpression::Not(expression) => Ok(!evaluate_filter(expression, entity)?),
    }
}

fn evaluate_comparison(
    left: &FilterOperand,
    operator: ComparisonOperator,
    right: &FilterOperand,
    entity: &impl EntityView,
) -> Result<bool, ValidationError> {
    let Some(left) = resolve_operand(left, entity)? else {
        return Ok(false);
    };
    let Some(right) = resolve_operand(right, entity)? else {
        return Ok(false);
    };

    compare_values(&left, operator, &right)
}

fn resolve_operand(
    operand: &FilterOperand,
    entity: &impl EntityView,
) -> Result<Option<ComparableValue>, ValidationError> {
    match operand {
        FilterOperand::Literal(value) => Ok(Some(ComparableValue::from_literal(value.clone()))),
        FilterOperand::Property(name) => {
            if name.eq_ignore_ascii_case("PartitionKey") {
                return Ok(Some(ComparableValue::String(
                    entity.partition_key().to_owned(),
                )));
            }
            if name.eq_ignore_ascii_case("RowKey") {
                return Ok(Some(ComparableValue::String(entity.row_key().to_owned())));
            }
            if name.eq_ignore_ascii_case("Timestamp") {
                return Ok(entity.timestamp().map(ComparableValue::DateTime));
            }

            Ok(entity.property(name).map(ComparableValue::from_property))
        }
    }
}

fn compare_values(
    left: &ComparableValue,
    operator: ComparisonOperator,
    right: &ComparableValue,
) -> Result<bool, ValidationError> {
    let ordering = match (left, right) {
        (ComparableValue::String(left), ComparableValue::String(right)) => Some(left.cmp(right)),
        (ComparableValue::Bool(left), ComparableValue::Bool(right)) => Some(left.cmp(right)),
        (ComparableValue::Int(left), ComparableValue::Int(right)) => Some(left.cmp(right)),
        (ComparableValue::Double(left), ComparableValue::Double(right)) => left.partial_cmp(right),
        (ComparableValue::DateTime(left), ComparableValue::DateTime(right)) => {
            Some(left.cmp(right))
        }
        (ComparableValue::Guid(left), ComparableValue::Guid(right)) => {
            Some(left.as_bytes().cmp(right.as_bytes()))
        }
        (ComparableValue::Binary(left), ComparableValue::Binary(right)) => Some(left.cmp(right)),
        _ => {
            return Err(ValidationError::InvalidQuery(
                "filter comparison uses incompatible operand types".to_owned(),
            ));
        }
    }
    .ok_or_else(|| ValidationError::InvalidQuery("invalid filter comparison".to_owned()))?;

    Ok(match operator {
        ComparisonOperator::Eq => ordering.is_eq(),
        ComparisonOperator::Ne => !ordering.is_eq(),
        ComparisonOperator::Gt => ordering.is_gt(),
        ComparisonOperator::Ge => ordering.is_ge(),
        ComparisonOperator::Lt => ordering.is_lt(),
        ComparisonOperator::Le => ordering.is_le(),
    })
}

#[derive(Debug, Clone, PartialEq)]
enum ComparableValue {
    String(String),
    Bool(bool),
    Int(i64),
    Double(f64),
    DateTime(OffsetDateTime),
    Guid(Uuid),
    Binary(Vec<u8>),
}

impl ComparableValue {
    fn from_property(property: &EntityProperty) -> Self {
        match property {
            EntityProperty::String(value) => Self::String(value.clone()),
            EntityProperty::Bool(value) => Self::Bool(*value),
            EntityProperty::Int32(value) => Self::Int(i64::from(*value)),
            EntityProperty::Int64(value) => Self::Int(*value),
            EntityProperty::Double(value) => Self::Double(*value),
            EntityProperty::Binary(value) => Self::Binary(value.clone()),
            EntityProperty::Guid(value) => Self::Guid(*value),
            EntityProperty::DateTime(value) => Self::DateTime(*value),
        }
    }

    fn from_literal(literal: FilterLiteral) -> Self {
        match literal {
            FilterLiteral::String(value) => Self::String(value),
            FilterLiteral::Bool(value) => Self::Bool(value),
            FilterLiteral::Int(value) => Self::Int(value),
            FilterLiteral::Double(value) => Self::Double(value),
            FilterLiteral::DateTime(value) => Self::DateTime(value),
            FilterLiteral::Guid(value) => Self::Guid(value),
            FilterLiteral::Binary(value) => Self::Binary(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Identifier(String),
    Literal(FilterLiteral),
    LParen,
    RParen,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    And,
    Or,
    Not,
    End,
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    current: Token,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Result<Self, ValidationError> {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token()?;
        Ok(Self { lexer, current })
    }

    fn parse_expression(&mut self) -> Result<FilterExpression, ValidationError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<FilterExpression, ValidationError> {
        let mut expression = self.parse_and()?;
        while matches!(self.current.kind, TokenKind::Or) {
            self.bump()?;
            let right = self.parse_and()?;
            expression = FilterExpression::Or(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<FilterExpression, ValidationError> {
        let mut expression = self.parse_not()?;
        while matches!(self.current.kind, TokenKind::And) {
            self.bump()?;
            let right = self.parse_not()?;
            expression = FilterExpression::And(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_not(&mut self) -> Result<FilterExpression, ValidationError> {
        if matches!(self.current.kind, TokenKind::Not) {
            self.bump()?;
            return Ok(FilterExpression::Not(Box::new(self.parse_not()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<FilterExpression, ValidationError> {
        if matches!(self.current.kind, TokenKind::LParen) {
            self.bump()?;
            let expression = self.parse_expression()?;
            self.expect(TokenKind::RParen)?;
            return Ok(expression);
        }

        let left = self.parse_operand()?;
        let operator = self.parse_comparison_operator()?;
        let right = self.parse_operand()?;

        Ok(FilterExpression::Comparison {
            left,
            operator,
            right,
        })
    }

    fn parse_operand(&mut self) -> Result<FilterOperand, ValidationError> {
        let operand = match &self.current.kind {
            TokenKind::Identifier(value) => FilterOperand::Property(value.clone()),
            TokenKind::Literal(value) => FilterOperand::Literal(value.clone()),
            _ => {
                return Err(ValidationError::InvalidQuery(
                    "unsupported filter operand".to_owned(),
                ));
            }
        };
        self.bump()?;
        Ok(operand)
    }

    fn parse_comparison_operator(&mut self) -> Result<ComparisonOperator, ValidationError> {
        let operator = match self.current.kind {
            TokenKind::Eq => ComparisonOperator::Eq,
            TokenKind::Ne => ComparisonOperator::Ne,
            TokenKind::Gt => ComparisonOperator::Gt,
            TokenKind::Ge => ComparisonOperator::Ge,
            TokenKind::Lt => ComparisonOperator::Lt,
            TokenKind::Le => ComparisonOperator::Le,
            _ => {
                return Err(ValidationError::InvalidQuery(
                    "unsupported filter operator".to_owned(),
                ));
            }
        };
        self.bump()?;
        Ok(operator)
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ValidationError> {
        if self.current.kind == kind {
            self.bump()?;
            Ok(())
        } else {
            Err(ValidationError::InvalidQuery(
                "unsupported filter syntax".to_owned(),
            ))
        }
    }

    fn bump(&mut self) -> Result<(), ValidationError> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, offset: 0 }
    }

    fn next_token(&mut self) -> Result<Token, ValidationError> {
        self.skip_whitespace();
        let Some(current) = self.peek_char() else {
            return Ok(Token {
                kind: TokenKind::End,
            });
        };

        let kind = match current {
            '(' => {
                self.offset += 1;
                TokenKind::LParen
            }
            ')' => {
                self.offset += 1;
                TokenKind::RParen
            }
            '\'' => TokenKind::Literal(FilterLiteral::String(self.read_quoted_string()?)),
            '-' | '0'..='9' => self.read_number()?,
            'A'..='Z' | 'a'..='z' | '_' => self.read_identifier_or_keyword()?,
            _ => {
                return Err(ValidationError::InvalidQuery(
                    "unsupported filter syntax".to_owned(),
                ));
            }
        };

        Ok(Token { kind })
    }

    fn read_identifier_or_keyword(&mut self) -> Result<TokenKind, ValidationError> {
        let start = self.offset;
        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.') {
                self.offset += ch.len_utf8();
            } else {
                break;
            }
        }

        let word = &self.input[start..self.offset];
        if self.peek_char() == Some('\'') {
            let raw = self.read_quoted_string()?;
            let literal = match word.to_ascii_lowercase().as_str() {
                "datetime" => {
                    FilterLiteral::DateTime(crate::codec::datetime::parse_datetime(&raw).map_err(
                        |_| ValidationError::InvalidQuery("invalid datetime literal".to_owned()),
                    )?)
                }
                "guid" => FilterLiteral::Guid(Uuid::parse_str(&raw).map_err(|_| {
                    ValidationError::InvalidQuery("invalid guid literal".to_owned())
                })?),
                "binary" => {
                    FilterLiteral::Binary(STANDARD.decode(raw.as_bytes()).map_err(|_| {
                        ValidationError::InvalidQuery("invalid binary literal".to_owned())
                    })?)
                }
                _ => {
                    return Err(ValidationError::InvalidQuery(
                        "unsupported typed filter literal".to_owned(),
                    ));
                }
            };
            return Ok(TokenKind::Literal(literal));
        }

        Ok(match word.to_ascii_lowercase().as_str() {
            "eq" => TokenKind::Eq,
            "ne" => TokenKind::Ne,
            "gt" => TokenKind::Gt,
            "ge" => TokenKind::Ge,
            "lt" => TokenKind::Lt,
            "le" => TokenKind::Le,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "true" => TokenKind::Literal(FilterLiteral::Bool(true)),
            "false" => TokenKind::Literal(FilterLiteral::Bool(false)),
            _ => TokenKind::Identifier(word.to_owned()),
        })
    }

    fn read_number(&mut self) -> Result<TokenKind, ValidationError> {
        let start = self.offset;
        if self.peek_char() == Some('-') {
            self.offset += 1;
        }
        while matches!(self.peek_char(), Some('0'..='9')) {
            self.offset += 1;
        }
        let mut is_double = false;
        if self.peek_char() == Some('.') {
            is_double = true;
            self.offset += 1;
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.offset += 1;
            }
        }
        if matches!(self.peek_char(), Some('e' | 'E')) {
            is_double = true;
            self.offset += 1;
            if matches!(self.peek_char(), Some('+' | '-')) {
                self.offset += 1;
            }
            while matches!(self.peek_char(), Some('0'..='9')) {
                self.offset += 1;
            }
        }

        let raw = &self.input[start..self.offset];
        if is_double {
            Ok(TokenKind::Literal(FilterLiteral::Double(
                raw.parse::<f64>().map_err(|_| {
                    ValidationError::InvalidQuery("invalid numeric literal".to_owned())
                })?,
            )))
        } else {
            Ok(TokenKind::Literal(FilterLiteral::Int(
                raw.parse::<i64>().map_err(|_| {
                    ValidationError::InvalidQuery("invalid integer literal".to_owned())
                })?,
            )))
        }
    }

    fn read_quoted_string(&mut self) -> Result<String, ValidationError> {
        self.consume_char('\'')?;
        let mut result = String::new();
        loop {
            let Some(ch) = self.peek_char() else {
                return Err(ValidationError::InvalidQuery(
                    "unterminated string literal".to_owned(),
                ));
            };
            self.offset += ch.len_utf8();
            if ch == '\'' {
                if self.peek_char() == Some('\'') {
                    self.offset += 1;
                    result.push('\'');
                    continue;
                }
                break;
            }
            result.push(ch);
        }
        Ok(result)
    }

    fn consume_char(&mut self, expected: char) -> Result<(), ValidationError> {
        match self.peek_char() {
            Some(ch) if ch == expected => {
                self.offset += ch.len_utf8();
                Ok(())
            }
            _ => Err(ValidationError::InvalidQuery(
                "unsupported filter syntax".to_owned(),
            )),
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_char(), Some(ch) if ch.is_ascii_whitespace()) {
            self.offset += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use crate::{entity::EntityProperty, query::filter::parse_filter};

    use super::{DynamicEntity, evaluate_filter};

    #[test]
    fn parses_and_evaluates_nested_filter() {
        let filter =
            parse_filter("(PartitionKey eq 'p1' and count ge 2) or not (active eq false)").unwrap();
        let mut entity = DynamicEntity::new("p1", "r1");
        entity.insert_property("count", EntityProperty::Int32(2));
        entity.insert_property("active", EntityProperty::Bool(true));
        entity.timestamp = Some(datetime!(2026-03-19 12:00:00 UTC));

        assert!(evaluate_filter(&filter, &entity).unwrap());
    }

    #[test]
    fn handles_missing_properties_as_false() {
        let filter = parse_filter("missing eq 'x'").unwrap();
        let entity = DynamicEntity::new("p1", "r1");

        assert!(!evaluate_filter(&filter, &entity).unwrap());
    }
}
