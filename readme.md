# laggyboi
## what is this?
you are writing a server/client application and use the same computer when testing the client and server.
You're app runs phenomenally and FAST in your testing but users are consistently running into edgecases you hadn't noticed when running yourself.
You need to introduce lag when testing to properly experience your users' experience, but don't want to set up any fancy network conditioners and hardware to simulate the connection.
Laggy boi provides a tcp tunnel that introduces lag.
`laggyboi -d 127.0.0.1:4269 -u 127.0.0.1:8080 -l 500`
assuming your server is listening on `127.0.0.1:8080`, point your client at `127.0.0.1:4269` and experience 500ms of bi directional lag.

## how to build
to build and run debug version with flags
`cargo run -- -d 127.0.0.1:4269 -u 127.0.0.1:8080 -l 500`