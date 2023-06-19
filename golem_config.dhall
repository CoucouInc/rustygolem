let StreamSpec: Type =
  { nickname: Text
  , irc_nick: Text
  , irc_channels: List Text
  }

let twitch =
  { client_id = env:TWITCH_CLIENT_ID as Text
  , client_secret = env:TWITCH_CLIENT_SECRET as Text
  -- the app_secret is used to verify webhook notifications
  -- coming from the twitch servers
  , app_secret = env:TWITCH_APP_SECRET as Text
  , server_bind_address = env:SERVER_BIND_ADDRESS ? "0.0.0.0"
  , server_bind_port = env:SERVER_BIND_PORT ? 7777
  , callback_uri = "https://irc.geekingfrog.com/touitche/coucou"
  , watched_streams = [
    { nickname = "artart78"
    , irc_nick = "artart78"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "gikiam"
    , irc_nick = "jiquiame"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "shampooingonthemove"
    , irc_nick = "Shampooing"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "vertbrocoli"
    , irc_nick = "Armael"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "therealbarul"
    , irc_nick = "barul"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "juantitor"
    , irc_nick = "Juantitor"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "chouhartem"
    , irc_nick = "Chouhartem"
    , irc_channels = ["##arch-fr-free"]
    },
    { nickname = "geekingfrog"
    , irc_nick = "Geekingfrog"
    , irc_channels = ["##arch-fr-free"]
    },
  ] : List StreamSpec
  }

in
{ twitch = twitch
-- these users will be ignored
-- Will need to figure out a way to bypass that somehow when implementing λurl
, blacklisted_users = ["coucoubot", "lambdacoucou", "M`arch`ov", "coucoucou"]
, sasl_password = Some (env:SASL_PASSWORD as Text) ? None Text
-- ctcp plugin is *required* to handle pings
, plugins = ["crypto", "twitch", "joke", "ctcp", "republican_calendar", "url"]
, youtube_api_key = Some (env:YT_API_KEY as Text) ? None Text
}
