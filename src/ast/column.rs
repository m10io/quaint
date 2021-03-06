use crate::ast::{DatabaseValue, Table};
use std::borrow::Cow;

/// A column definition.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Column<'a> {
    pub name: Cow<'a, str>,
    pub(crate) table: Option<Table<'a>>,
    pub(crate) alias: Option<Cow<'a, str>>,
}

#[macro_export]
/// Marks a given string or a tuple as a column. Useful when using a column in
/// calculations, e.g.
///
/// ``` rust
/// # use quaint::{col, val, ast::*, visitor::{Visitor, Sqlite}};
/// let join = "dogs".on(("dogs", "slave_id").equals(Column::from(("cats", "master_id"))));
///
/// let query = Select::from_table("cats")
///     .value(Table::from("cats").asterisk())
///     .value(col!("dogs", "age") - val!(4))
///     .inner_join(join);
///
/// let (sql, params) = Sqlite::build(query);
///
/// assert_eq!(
///     "SELECT `cats`.*, (`dogs`.`age` - ?) FROM `cats` INNER JOIN `dogs` ON `dogs`.`slave_id` = `cats`.`master_id`",
///     sql
/// );
/// ```
macro_rules! col {
    ($e1:expr) => {
        DatabaseValue::from(Column::from($e1))
    };

    ($e1:expr, $e2:expr) => {
        DatabaseValue::from(Column::from(($e1, $e2)))
    };
}

impl<'a> From<Column<'a>> for DatabaseValue<'a> {
    #[inline]
    fn from(col: Column<'a>) -> Self {
        DatabaseValue::Column(Box::new(col))
    }
}

impl<'a> Column<'a> {
    /// Create a column definition.
    #[inline]
    pub fn new<S>(name: S) -> Self
    where
        S: Into<Cow<'a, str>>,
    {
        Column {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Include the table name in the column expression.
    #[inline]
    pub fn table<T>(mut self, table: T) -> Self
    where
        T: Into<Table<'a>>,
    {
        self.table = Some(table.into());
        self
    }

    /// Include the table name in the column expression, if table is defined.
    #[inline]
    pub fn opt_table<T>(mut self, table: Option<T>) -> Self
    where
        T: Into<Table<'a>>,
    {
        if let Some(table) = table {
            self.table = Some(table.into());
        }

        self
    }

    /// Give the column an alias in the query.
    #[inline]
    pub fn alias<S>(mut self, alias: S) -> Self
    where
        S: Into<Cow<'a, str>>,
    {
        self.alias = Some(alias.into());
        self
    }
}

impl<'a> From<&'a str> for Column<'a> {
    #[inline]
    fn from(s: &'a str) -> Self {
        Column {
            name: s.into(),
            ..Default::default()
        }
    }
}

impl<'a> From<String> for Column<'a> {
    #[inline]
    fn from(s: String) -> Self {
        Column {
            name: s.into(),
            ..Default::default()
        }
    }
}

impl<'a, T, C> From<(T, C)> for Column<'a>
where
    T: Into<Table<'a>>,
    C: Into<Column<'a>>,
{
    #[inline]
    fn from(t: (T, C)) -> Column<'a> {
        let mut column: Column<'a> = t.1.into();
        column = column.table(t.0);

        column
    }
}
