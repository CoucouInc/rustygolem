# RIIR !!!

nuff said

# Features ?

* Gives the current date in the [french republican calendar](https://en.wikipedia.org/wiki/French_Republican_calendar).
* Twitch integration to be notified when fellow chan members are streaming.
* Url grab to fetch the title with special integration for youtube API.
* Track the rates and evolution of various cryptoshitcoins.


# Migrations
Follow the [diesel getting started guide](https://diesel.rs/guides/getting-started.html).

Commands are roughly

```bash
export DATABASE_URL=rustygolem.sqlite
diesel migration generate <MIGRATION_NAME>
diesel migration run
diesel migration redo
```
