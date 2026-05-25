use crate::client::Client;
use crate::error::Error;
use crate::row::Row;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsolationLevel {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl IsolationLevel {
    fn as_sql(&self) -> &'static str {
        match self {
            IsolationLevel::ReadCommitted => "READ COMMITTED",
            IsolationLevel::RepeatableRead => "REPEATABLE READ",
            IsolationLevel::Serializable => "SERIALIZABLE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AccessMode {
    ReadWrite,
    ReadOnly,
}

impl AccessMode {
    fn as_sql(&self) -> &'static str {
        match self {
            AccessMode::ReadWrite => "READ WRITE",
            AccessMode::ReadOnly => "READ ONLY",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransactionOptions {
    pub isolation_level: Option<IsolationLevel>,
    pub access_mode: Option<AccessMode>,
    pub deferrable: Option<bool>,
}

impl Default for TransactionOptions {
    fn default() -> Self {
        TransactionOptions {
            isolation_level: None,
            access_mode: None,
            deferrable: None,
        }
    }
}

impl TransactionOptions {
    pub fn new() -> Self { Self::default() }

    pub fn isolation_level(mut self, level: IsolationLevel) -> Self {
        self.isolation_level = Some(level);
        self
    }

    pub fn read_only(mut self) -> Self {
        self.access_mode = Some(AccessMode::ReadOnly);
        self
    }

    pub fn deferrable(mut self) -> Self {
        self.deferrable = Some(true);
        self
    }

    fn to_begin_sql(&self) -> String {
        let mut parts = vec!["BEGIN".to_string()];
        if let Some(level) = &self.isolation_level {
            parts.push(format!("ISOLATION LEVEL {}", level.as_sql()));
        }
        if let Some(mode) = &self.access_mode {
            parts.push(mode.as_sql().to_string());
        }
        if let Some(d) = self.deferrable {
            if d { parts.push("DEFERRABLE".to_string()); }
            else { parts.push("NOT DEFERRABLE".to_string()); }
        }
        parts.join(" ")
    }
}

pub struct Transaction<'a> {
    client: &'a mut Client,
    finished: bool,
}

impl<'a> Transaction<'a> {
    pub async fn begin(client: &'a mut Client, options: TransactionOptions) -> Result<Self, Error> {
        let sql = options.to_begin_sql();
        client.execute(&sql).await?;
        Ok(Transaction { client, finished: false })
    }

    pub async fn commit(mut self) -> Result<(), Error> {
        self.client.execute("COMMIT").await?;
        self.finished = true;
        Ok(())
    }

    pub async fn rollback(mut self) -> Result<(), Error> {
        self.client.execute("ROLLBACK").await?;
        self.finished = true;
        Ok(())
    }

    pub async fn query(&mut self, sql: &str, params: &[&str]) -> Result<Vec<Row>, Error> {
        self.client.query(sql, params).await
    }

    pub async fn execute(&mut self, sql: &str) -> Result<u64, Error> {
        self.client.execute(sql).await
    }

    pub async fn savepoint(&mut self, name: &str) -> Result<Savepoint<'_>, Error> {
        Savepoint::create_inner(&mut *self.client, name).await
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.finished {
            log::warn!("Transaction dropped without commit or rollback — connection state may be undefined");
        }
    }
}

pub struct Savepoint<'a> {
    client: &'a mut Client,
    name: String,
    released: bool,
}

impl<'a> Savepoint<'a> {
    async fn create_inner(client: &'a mut Client, name: &str) -> Result<Self, Error> {
        let sql = format!("SAVEPOINT {}", crate::client::double_quote_identifier(name));
        client.execute(&sql).await?;
        Ok(Savepoint { client, name: name.to_string(), released: false })
    }

    pub async fn release(mut self) -> Result<(), Error> {
        let sql = format!("RELEASE SAVEPOINT {}", crate::client::double_quote_identifier(&self.name));
        self.client.execute(&sql).await?;
        self.released = true;
        Ok(())
    }

    pub async fn rollback(mut self) -> Result<(), Error> {
        let sql = format!("ROLLBACK TO SAVEPOINT {}", crate::client::double_quote_identifier(&self.name));
        self.client.execute(&sql).await?;
        self.released = true;
        Ok(())
    }
}

impl Drop for Savepoint<'_> {
    fn drop(&mut self) {
        if !self.released {
            log::warn!("Savepoint '{}' dropped without release or rollback", self.name);
        }
    }
}

impl Client {
    pub async fn begin(&mut self) -> Result<Transaction<'_>, Error> {
        Transaction::begin(self, TransactionOptions::default()).await
    }

    /// Run a closure in a transaction. Commits on `Ok`, rolls back on `Err`.
    pub async fn with_transaction<F, Fut, T>(&mut self, options: TransactionOptions, f: F) -> Result<T, Error>
    where
        F: FnOnce(&mut Transaction<'_>) -> Fut,
        Fut: std::future::Future<Output = Result<T, Error>>,
    {
        let mut txn = Transaction::begin(self, options).await?;
        match f(&mut txn).await {
            Ok(val) => { txn.commit().await?; Ok(val) }
            Err(e) => {
                let _ = txn.rollback().await;
                Err(e)
            }
        }
    }

    pub async fn commit(&mut self) -> Result<(), Error> { self.execute("COMMIT").await.map(|_| ()) }
    pub async fn rollback(&mut self) -> Result<(), Error> { self.execute("ROLLBACK").await.map(|_| ()) }
}
