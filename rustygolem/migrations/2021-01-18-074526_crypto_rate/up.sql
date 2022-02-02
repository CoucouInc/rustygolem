-- Your SQL goes here
CREATE TABLE IF NOT EXISTS crypto_rate (
  date DATETIME NOT NULL,
  coin TEXT NOT NULL,
  rate REAL NOT NULL,
  PRIMARY KEY(date, coin)
)
