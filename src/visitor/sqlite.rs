use crate::{ast::*, visitor::Visitor};

#[cfg(feature = "sqlite")]
use sqlite::{Bindable, Result as SqliteResult, Statement};

#[cfg(feature = "rusqlite")]
use rusqlite::{
    types::{Null, ToSql, ToSqlOutput},
    Error as RusqlError,
};

/// A visitor for generating queries for an SQLite database. Requires that
/// `rusqlite` feature flag is selected.
pub struct Sqlite {
    parameters: Vec<ParameterizedValue>,
}

impl Visitor for Sqlite {
    const C_BACKTICK: &'static str = "`";
    const C_WILDCARD: &'static str = "%";

    fn build<Q>(query: Q) -> (String, Vec<ParameterizedValue>)
    where
        Q: Into<Query>,
    {
        let mut sqlite = Sqlite {
            parameters: Vec::new(),
        };

        (
            Sqlite::visit_query(&mut sqlite, query.into()),
            sqlite.parameters,
        )
    }

    fn visit_insert(&mut self, insert: Insert) -> String {
        let mut result = match insert.on_conflict {
            Some(OnConflict::DoNothing) => vec![String::from("INSERT OR IGNORE")],
            None => vec![String::from("INSERT")],
        };

        result.push(format!("INTO {}", self.visit_table(insert.table, true)));

        if insert.values.is_empty() {
            result.push("DEFAULT VALUES".to_string());
        } else {
            let columns: Vec<String> = insert
                .columns
                .into_iter()
                .map(|c| self.visit_column(Column::from(c)))
                .collect();

            let values: Vec<String> = insert
                .values
                .into_iter()
                .map(|row| self.visit_row(row))
                .collect();

            result.push(format!(
                "({}) VALUES {}",
                columns.join(", "),
                values.join(", "),
            ))
        }

        result.join(" ")
    }

    fn parameter_substitution(&self) -> String {
        String::from("?")
    }

    fn add_parameter(&mut self, value: ParameterizedValue) {
        self.parameters.push(value);
    }

    fn visit_limit_and_offset(
        &mut self,
        limit: Option<ParameterizedValue>,
        offset: Option<ParameterizedValue>,
    ) -> Option<String> {
        match (limit, offset) {
            (Some(limit), Some(offset)) => Some(format!(
                "LIMIT {} OFFSET {}",
                self.visit_parameterized(limit),
                self.visit_parameterized(offset)
            )),
            (None, Some(offset)) => Some(format!(
                "LIMIT {} OFFSET {}",
                self.visit_parameterized(ParameterizedValue::from(-1)),
                self.visit_parameterized(offset)
            )),
            (Some(limit), None) => Some(format!("LIMIT {}", self.visit_parameterized(limit))),
            (None, None) => None,
        }
    }

    fn visit_aggregate_to_string(&mut self, value: DatabaseValue) -> String {
        format!("group_concat({})", self.visit_database_value(value))
    }
}

#[cfg(feature = "sqlite")]
impl Bindable for ParameterizedValue {
    #[inline]
    fn bind(self, statement: &mut Statement, i: usize) -> SqliteResult<()> {
        use ParameterizedValue as Pv;
        match self {
            Pv::Null => statement.bind(i, ()),
            Pv::Integer(integer) => statement.bind(i, integer),
            Pv::Real(float) => statement.bind(i, float),
            Pv::Text(string) => statement.bind(i, string.as_str()),

            // Sqlite3 doesn't have booleans so we match to ints
            Pv::Boolean(true) => statement.bind(i, 1),
            Pv::Boolean(false) => statement.bind(i, 0),
        }
    }
}

// TODO: This most likely should be in another class, as it is not related to sqlite.
#[cfg(feature = "rusqlite")]
impl ToSql for ParameterizedValue {
    fn to_sql(&self) -> Result<ToSqlOutput, RusqlError> {
        let value = match self {
            ParameterizedValue::Null => ToSqlOutput::from(Null),
            ParameterizedValue::Integer(integer) => ToSqlOutput::from(*integer),
            ParameterizedValue::Real(float) => ToSqlOutput::from(*float),
            ParameterizedValue::Text(string) => ToSqlOutput::from(string.clone()),
            ParameterizedValue::Boolean(boo) => ToSqlOutput::from(*boo),
            #[cfg(feature = "array")]
            ParameterizedValue::Array(vec) => ToSqlOutput::from(vec),
            #[cfg(feature = "json-1")]
            ParameterizedValue::Json(value) => {
                let stringified = serde_json::to_string(value)
                    .map_err(|err| RusqlError::ToSqlConversionFailure(Box::new(err)))?;
                ToSqlOutput::from(stringified)
            }
            #[cfg(feature = "uuid-0_7")]
            ParameterizedValue::Uuid(value) => ToSqlOutput::from(value.to_hyphenated().to_string()),
            #[cfg(feature = "chrono-0_4")]
            ParameterizedValue::DateTime(value) => ToSqlOutput::from(value.timestamp_millis()),
        };

        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::visitor::*;

    fn expected_values<T>(sql: &'static str, params: Vec<T>) -> (String, Vec<ParameterizedValue>)
    where
        T: Into<ParameterizedValue>,
    {
        (
            String::from(sql),
            params.into_iter().map(|p| p.into()).collect(),
        )
    }

    fn default_params(mut additional: Vec<ParameterizedValue>) -> Vec<ParameterizedValue> {
        let mut result = Vec::new();

        for param in additional.drain(0..) {
            result.push(param)
        }

        result
    }

    #[test]
    fn test_select_1() {
        let expected = expected_values("SELECT ?", vec![1]);

        let query = Select::default().value(1);
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(expected.1, params);
    }

    #[test]
    fn test_select_star_from() {
        let expected_sql = "SELECT `musti`.* FROM `musti`";
        let query = Select::from_table("musti");
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(vec![]), params);
    }

    #[test]
    fn test_select_order_by() {
        let expected_sql = "SELECT `musti`.* FROM `musti` ORDER BY `foo`, `baz` ASC, `bar` DESC";
        let query = Select::from_table("musti")
            .order_by("foo")
            .order_by("baz".ascend())
            .order_by("bar".descend());
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(vec![]), params);
    }

    #[test]
    fn test_select_fields_from() {
        let expected_sql = "SELECT `paw`, `nose` FROM `cat`.`musti`";
        let query = Select::from_table(("cat", "musti"))
            .column("paw")
            .column("nose");
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(vec![]), params);
    }

    #[test]
    fn test_select_where_equals() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` = ?",
            vec!["meow"],
        );

        let query = Select::from_table("naukio").so_that("word".equals("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_where_like() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` LIKE ?",
            vec!["%meow%"],
        );

        let query = Select::from_table("naukio").so_that("word".like("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_where_not_like() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` NOT LIKE ?",
            vec!["%meow%"],
        );

        let query = Select::from_table("naukio").so_that("word".not_like("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_where_begins_with() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` LIKE ?",
            vec!["meow%"],
        );

        let query = Select::from_table("naukio").so_that("word".begins_with("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_where_not_begins_with() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` NOT LIKE ?",
            vec!["meow%"],
        );

        let query = Select::from_table("naukio").so_that("word".not_begins_with("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_where_ends_into() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` LIKE ?",
            vec!["%meow"],
        );

        let query = Select::from_table("naukio").so_that("word".ends_into("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_where_not_ends_into() {
        let expected = expected_values(
            "SELECT `naukio`.* FROM `naukio` WHERE `word` NOT LIKE ?",
            vec!["%meow"],
        );

        let query = Select::from_table("naukio").so_that("word".not_ends_into("meow"));
        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected.0, sql);
        assert_eq!(default_params(expected.1), params);
    }

    #[test]
    fn test_select_and() {
        let expected_sql =
            "SELECT `naukio`.* FROM `naukio` WHERE ((`word` = ? AND `age` < ?) AND `paw` = ?)";

        let expected_params = vec![
            ParameterizedValue::Text(String::from("meow")),
            ParameterizedValue::Integer(10),
            ParameterizedValue::Text(String::from("warm")),
        ];

        let conditions = "word"
            .equals("meow")
            .and("age".less_than(10))
            .and("paw".equals("warm"));

        let query = Select::from_table("naukio").so_that(conditions);

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(expected_params), params);
    }

    #[test]
    fn test_select_and_different_execution_order() {
        let expected_sql =
            "SELECT `naukio`.* FROM `naukio` WHERE (`word` = ? AND (`age` < ? AND `paw` = ?))";

        let expected_params = vec![
            ParameterizedValue::Text(String::from("meow")),
            ParameterizedValue::Integer(10),
            ParameterizedValue::Text(String::from("warm")),
        ];

        let conditions = "word"
            .equals("meow")
            .and("age".less_than(10).and("paw".equals("warm")));

        let query = Select::from_table("naukio").so_that(conditions);

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(expected_params), params);
    }

    #[test]
    fn test_select_or() {
        let expected_sql =
            "SELECT `naukio`.* FROM `naukio` WHERE ((`word` = ? OR `age` < ?) AND `paw` = ?)";

        let expected_params = vec![
            ParameterizedValue::Text(String::from("meow")),
            ParameterizedValue::Integer(10),
            ParameterizedValue::Text(String::from("warm")),
        ];

        let conditions = "word"
            .equals("meow")
            .or("age".less_than(10))
            .and("paw".equals("warm"));

        let query = Select::from_table("naukio").so_that(conditions);

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(expected_params), params);
    }

    #[test]
    fn test_select_negation() {
        let expected_sql =
            "SELECT `naukio`.* FROM `naukio` WHERE (NOT ((`word` = ? OR `age` < ?) AND `paw` = ?))";

        let expected_params = vec![
            ParameterizedValue::Text(String::from("meow")),
            ParameterizedValue::Integer(10),
            ParameterizedValue::Text(String::from("warm")),
        ];

        let conditions = "word"
            .equals("meow")
            .or("age".less_than(10))
            .and("paw".equals("warm"))
            .not();

        let query = Select::from_table("naukio").so_that(conditions);

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(expected_params), params);
    }

    #[test]
    fn test_with_raw_condition_tree() {
        let expected_sql =
            "SELECT `naukio`.* FROM `naukio` WHERE (NOT ((`word` = ? OR `age` < ?) AND `paw` = ?))";

        let expected_params = vec![
            ParameterizedValue::Text(String::from("meow")),
            ParameterizedValue::Integer(10),
            ParameterizedValue::Text(String::from("warm")),
        ];

        let conditions = ConditionTree::not(ConditionTree::and(
            ConditionTree::or("word".equals("meow"), "age".less_than(10)),
            "paw".equals("warm"),
        ));

        let query = Select::from_table("naukio").so_that(conditions);

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(default_params(expected_params), params);
    }

    #[test]
    fn test_simple_inner_join() {
        let expected_sql =
            "SELECT `users`.* FROM `users` INNER JOIN `posts` ON `users`.`id` = `posts`.`user_id`";

        let query = Select::from_table("users")
            .inner_join("posts".on(("users", "id").equals(Column::from(("posts", "user_id")))));
        let (sql, _) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
    }

    #[test]
    fn test_additional_condition_inner_join() {
        let expected_sql =
            "SELECT `users`.* FROM `users` INNER JOIN `posts` ON (`users`.`id` = `posts`.`user_id` AND `posts`.`published` = ?)";

        let query = Select::from_table("users").inner_join(
            "posts".on(("users", "id")
                .equals(Column::from(("posts", "user_id")))
                .and(("posts", "published").equals(true))),
        );

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(
            default_params(vec![ParameterizedValue::Boolean(true),]),
            params
        );
    }

    #[test]
    fn test_simple_left_join() {
        let expected_sql =
            "SELECT `users`.* FROM `users` LEFT OUTER JOIN `posts` ON `users`.`id` = `posts`.`user_id`";

        let query = Select::from_table("users").left_outer_join(
            "posts".on(("users", "id").equals(Column::from(("posts", "user_id")))),
        );
        let (sql, _) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
    }

    #[test]
    fn test_additional_condition_left_join() {
        let expected_sql =
            "SELECT `users`.* FROM `users` LEFT OUTER JOIN `posts` ON (`users`.`id` = `posts`.`user_id` AND `posts`.`published` = ?)";

        let query = Select::from_table("users").left_outer_join(
            "posts".on(("users", "id")
                .equals(Column::from(("posts", "user_id")))
                .and(("posts", "published").equals(true))),
        );

        let (sql, params) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
        assert_eq!(
            default_params(vec![ParameterizedValue::Boolean(true),]),
            params
        );
    }

    #[test]
    fn test_column_aliasing() {
        let expected_sql = "SELECT `bar` AS `foo` FROM `meow`";
        let query = Select::from_table("meow").column(Column::new("bar").alias("foo"));
        let (sql, _) = Sqlite::build(query);

        assert_eq!(expected_sql, sql);
    }

    /// Creates a simple sqlite database with a user table and a nice user
    #[cfg(feature = "sqlite")]
    fn sqlite_harness() -> ::sqlite::Connection {
        let conn = ::sqlite::open(":memory:").unwrap();

        conn.execute(
            "
            CREATE TABLE users (id, name TEXT, age REAL, nice INTEGER);
            INSERT INTO users (id, name, age, nice) VALUES (1, 'Alice', 42.69, 1);
            ",
        )
        .unwrap();

        conn
    }

    #[test]
    #[cfg(feature = "sqlite")]
    fn bind_test_1() {
        let conn = sqlite_harness();

        let conditions = "name"
            .equals("Alice")
            .and("age".less_than(100.0))
            .and("nice".equals(true));
        let query = Select::from_table("users").so_that(conditions);
        let (sql_str, params) = Sqlite::build(query);

        let mut s = conn.prepare(sql_str.clone()).unwrap();
        for i in 1..params.len() + 1 {
            s.bind::<ParameterizedValue>(i, params[i - 1].clone().into())
                .unwrap();
        }

        s.next().unwrap();

        assert_eq!("Alice", s.read::<String>(1).unwrap());
        assert_eq!(42.69, s.read::<f64>(2).unwrap());
        assert_eq!(1, s.read::<i64>(3).unwrap());
    }

    #[cfg(feature = "rusqlite")]
    fn sqlite_harness() -> ::rusqlite::Connection {
        let conn = ::rusqlite::Connection::open_in_memory().unwrap();

        conn.execute(
            "CREATE TABLE users (id, name TEXT, age REAL, nice INTEGER)",
            ::rusqlite::NO_PARAMS,
        )
        .unwrap();

        let insert = Insert::single_into("users")
            .value("id", 1)
            .value("name", "Alice")
            .value("age", 42.69)
            .value("nice", true);

        let (sql, params) = dbg!(Sqlite::build(insert));

        conn.execute(&sql, params.as_slice()).unwrap();
        conn
    }

    #[test]
    #[cfg(feature = "rusqlite")]
    fn bind_test_1() {
        let conn = sqlite_harness();

        let conditions = "name"
            .equals("Alice")
            .and("age".less_than(100.0))
            .and("nice".equals(1));
        let query = Select::from_table("users").so_that(conditions);
        let (sql_str, params) = Sqlite::build(query);

        #[derive(Debug)]
        struct Person {
            name: String,
            age: f64,
            nice: i32,
        }

        let mut stmt = conn.prepare(&sql_str).unwrap();
        let mut person_iter = stmt
            .query_map(&params, |row| {
                Ok(Person {
                    name: row.get(1).unwrap(),
                    age: row.get(2).unwrap(),
                    nice: row.get(3).unwrap(),
                })
            })
            .unwrap();

        let person: Person = person_iter.nth(0).unwrap().unwrap();

        assert_eq!("Alice", person.name);
        assert_eq!(42.69, person.age);
        assert_eq!(1, person.nice);
    }
}
