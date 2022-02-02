ALTER TABLE crypto_rate RENAME TO crypto_rate_tmp;
CREATE TABLE crypto_rate (
  date DATETIME NOT NULL,
  coin TEXT CHECK(coin in ("BTC", "ETH", "DOGE", "XRP", "ALGO")) NOT NULL,
  rate REAL NOT NULL,
  PRIMARY KEY(date, coin)
);


INSERT INTO crypto_rate SELECT * FROM crypto_rate_tmp;
DROP TABLE crypto_rate_tmp;
