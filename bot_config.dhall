let StreamSpec : Type
  = { -- the twitch nickname. what's in the stream url
      nickname: Text,
      -- the irc nickname of the owner of that stream
      irc_nick: Text,
      -- which channels to notify?
      irc_channels: List Text
  }
in
{ twitch_module =
    { client_id = env:TWITCH_CLIENT_ID as Text
    , client_secret = env:TWITCH_CLIENT_SECRET as Text
    , watched_streams = [
      { nickname = "geekingfrog"
      , irc_nick = "Geekingfrog"
      , irc_channels = ["##gougoutest"]
      }]
    }
, more_flag= "coucou"
}
