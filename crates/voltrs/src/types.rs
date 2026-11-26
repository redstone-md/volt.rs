//! Rust to TypeScript type mapping used to generate the client SDK.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

/// Collects named TypeScript declarations while types are being described.
#[derive(Debug, Default)]
pub struct TypeScript {
    decls: BTreeMap<String, Option<String>>,
}

impl TypeScript {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a named declaration once and returns its name. Recursive types are safe.
    pub fn named(&mut self, name: &str, build: impl FnOnce(&mut Self) -> String) -> String {
        if !self.decls.contains_key(name) {
            self.decls.insert(name.to_owned(), None);
            let decl = build(self);
            self.decls.insert(name.to_owned(), Some(decl));
        }
        name.to_owned()
    }

    pub fn declarations(&self) -> impl Iterator<Item = &str> {
        self.decls.values().filter_map(|d| d.as_deref())
    }

    pub fn describe<T: Type + ?Sized>(&mut self) -> String {
        T::ts(self)
    }
}

/// A type with a TypeScript representation. Derive it with `#[derive(voltrs::Type)]`
/// or `#[voltrs::model]`.
pub trait Type {
    fn ts(ts: &mut TypeScript) -> String;
}

/// Wraps unions so they stay valid inside array types.
pub fn array_of(inner: &str) -> String {
    if inner.contains(' ') { format!("({inner})[]") } else { format!("{inner}[]") }
}

macro_rules! primitive {
    ($ts:literal => $($ty:ty),+) => {
        $(impl Type for $ty {
            fn ts(_: &mut TypeScript) -> String { $ts.into() }
        })+
    };
}

primitive!("number" => u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, f32, f64);
primitive!("string" => String, str, char);
primitive!("boolean" => bool);
primitive!("null" => ());
primitive!("unknown" => serde_json::Value);

impl<T: Type + ?Sized> Type for &T {
    fn ts(ts: &mut TypeScript) -> String {
        T::ts(ts)
    }
}

macro_rules! transparent {
    ($($wrapper:ident),+) => {
        $(impl<T: Type + ?Sized> Type for $wrapper<T> {
            fn ts(ts: &mut TypeScript) -> String { T::ts(ts) }
        })+
    };
}

transparent!(Box, Arc, Rc);

impl<T: Type> Type for Option<T> {
    fn ts(ts: &mut TypeScript) -> String {
        format!("{} | null", T::ts(ts))
    }
}

macro_rules! list {
    ($($list:ident),+) => {
        $(impl<T: Type> Type for $list<T> {
            fn ts(ts: &mut TypeScript) -> String { array_of(&T::ts(ts)) }
        })+
    };
}

list!(Vec, HashSet, BTreeSet);

impl<T: Type> Type for [T] {
    fn ts(ts: &mut TypeScript) -> String {
        array_of(&T::ts(ts))
    }
}

impl<T: Type, const N: usize> Type for [T; N] {
    fn ts(ts: &mut TypeScript) -> String {
        array_of(&T::ts(ts))
    }
}

impl<K, V: Type, S> Type for HashMap<K, V, S> {
    fn ts(ts: &mut TypeScript) -> String {
        format!("Record<string, {}>", V::ts(ts))
    }
}

impl<K, V: Type> Type for BTreeMap<K, V> {
    fn ts(ts: &mut TypeScript) -> String {
        format!("Record<string, {}>", V::ts(ts))
    }
}

macro_rules! tuple {
    ($($name:ident),+) => {
        impl<$($name: Type),+> Type for ($($name,)+) {
            fn ts(ts: &mut TypeScript) -> String {
                let items: Vec<String> = vec![$($name::ts(ts)),+];
                format!("[{}]", items.join(", "))
            }
        }
    };
}

tuple!(A);
tuple!(A, B);
tuple!(A, B, C);
tuple!(A, B, C, D);

/// Converts a Rust identifier to camelCase.
pub fn camel_case(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len());
    let mut upper = false;
    for (i, c) in ident.chars().enumerate() {
        if c == '_' {
            upper = i > 0;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_std_types() {
        let mut ts = TypeScript::new();
        assert_eq!(ts.describe::<Vec<Option<u32>>>(), "(number | null)[]");
        assert_eq!(ts.describe::<HashMap<String, bool>>(), "Record<string, boolean>");
        assert_eq!(ts.describe::<(String, i64)>(), "[string, number]");
        assert_eq!(camel_case("add_todo_item"), "addTodoItem");
    }
}
