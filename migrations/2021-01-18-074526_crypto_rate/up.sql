-- Your SQL goes here
CREATE TABLE IF NOT EXISTS crypto_rate (
  date DATETIME NOT NULL,
  coin TEXT CHECK(coin IN ("BTC", "ETH")) NOT NULL,
  rate REAL NOT NULL,
  PRIMARY KEY(date, coin)
)
