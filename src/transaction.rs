use crate::client::Client;
use crate::error::Error;

impl Client {
    pub async fn begin(&mut self) -> Result<(), Error> {
        self.execute("BEGIN").await.map(|_| ())
    }

    pub async fn commit(&mut self) -> Result<(), Error> {
        self.execute("COMMIT").await.map(|_| ())
    }

    pub async fn rollback(&mut self) -> Result<(), Error> {
        self.execute("ROLLBACK").await.map(|_| ())
    }
}
