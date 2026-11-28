A binding assigned `http.request(...)` or `https.request(...)` after its declaration is a `ClientRequest`, so `client.end()` sends the request instead of returning the bound method.
