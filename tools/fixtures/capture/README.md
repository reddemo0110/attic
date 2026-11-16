Pages for web-capture checks without a network.

`JJB_CAPTURE_FIXTURE=tools/fixtures/capture` makes the app read these
instead of going online: `https://fixture.test/story.html` is a
three-page article ("Page 1 of 3"), `https://fixture.test/paid.html` a
login wall, and any other address fails as a missing page would.
