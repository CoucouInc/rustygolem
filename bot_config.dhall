let StreamSpec: Type =
  { nickname: Text
  , irc_nick: Text
  , irc_channels: List Text
  }

in
{ twitch_module =
    { client_id = env:TWITCH_CLIENT_ID as Text
    , client_secret = env:TWITCH_CLIENT_SECRET as Text
    -- the app_secret is used to verify webhook notifications
    -- coming from the twitch servers
    , app_secret = env:TWITCH_APP_SECRET as Text
    , webhook_bind = env:WEBHOOK_BIND ? "0.0.0.0"
    , webhook_port = env:WEBHOOK_PORT ? 7777
    , callback_uri = "https://irc.geekingfrog.com/touitche/coucou"
    , watched_streams = [
      { nickname = "geekingfrog"
      , irc_nick = "Geekingfrog"
      , irc_channels = ["##arch-fr-free"]
      }] : List StreamSpec
    }
, blacklisted_users = ["coucoubot", "lambdacoucou", "M`arch`ov", "coucoucou"]
, sasl_password = Some (env:SASL_PASSWORD as Text) ? None Text
}
