use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use thiserror::Error;

/// A sequence guaranteed non-empty at the type level.
///
/// The empty case is unrepresentable: every value has a `head`. Multi-element
/// sequences extend through `tail`. Construction from an external sequence
/// fails closed with [`NonEmptyError::Empty`].
#[derive(Archive, RkyvSerialize, RkyvDeserialize, Debug, Clone, PartialEq, Eq)]
pub struct NonEmpty<Value> {
    head: Value,
    tail: Vec<Value>,
}

impl<Value> NonEmpty<Value> {
    pub fn single(head: Value) -> Self {
        Self {
            head,
            tail: Vec::new(),
        }
    }

    pub fn from_head_and_tail(head: Value, tail: Vec<Value>) -> Self {
        Self { head, tail }
    }

    pub fn try_from_vec(mut values: Vec<Value>) -> Result<Self, NonEmptyError> {
        if values.is_empty() {
            return Err(NonEmptyError::Empty);
        }
        let head = values.remove(0);
        Ok(Self { head, tail: values })
    }

    pub fn push(&mut self, value: Value) {
        self.tail.push(value);
    }

    pub fn head(&self) -> &Value {
        &self.head
    }

    pub fn tail(&self) -> &[Value] {
        &self.tail
    }

    pub fn len(&self) -> usize {
        1 + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn iter(&self) -> NonEmptyIterator<'_, Value> {
        NonEmptyIterator {
            head: Some(&self.head),
            tail: self.tail.iter(),
        }
    }

    pub fn into_vec(self) -> Vec<Value> {
        let mut values = Vec::with_capacity(1 + self.tail.len());
        values.push(self.head);
        values.extend(self.tail);
        values
    }

    pub fn into_head(self) -> Value {
        self.head
    }

    pub fn into_head_and_tail(self) -> (Value, Vec<Value>) {
        (self.head, self.tail)
    }
}

impl<Value> IntoIterator for NonEmpty<Value> {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_vec().into_iter()
    }
}

impl<'value, Value> IntoIterator for &'value NonEmpty<Value> {
    type Item = &'value Value;
    type IntoIter = NonEmptyIterator<'value, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct NonEmptyIterator<'value, Value> {
    head: Option<&'value Value>,
    tail: std::slice::Iter<'value, Value>,
}

impl<'value, Value> Iterator for NonEmptyIterator<'value, Value> {
    type Item = &'value Value;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(head) = self.head.take() {
            return Some(head);
        }
        self.tail.next()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum NonEmptyError {
    #[error("cannot construct a NonEmpty from an empty sequence")]
    Empty,
}
