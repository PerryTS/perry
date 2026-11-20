"use strict";
// Vendored from mongodb-connection-string-url 7.0.2 lib/index.js (lines
// 73-130, trimmed): the subclass reaches whatwg-url's `URL` as a MEMBER of
// the require() namespace, first through an implicit-constructor class, then
// through a class with a field and `super(...)` inside `try`.
Object.defineProperty(exports, "__esModule", { value: true });
const whatwg_url_1 = require("./whatwg_url.cjs");
const DUMMY_HOSTNAME = "__this_is_a_placeholder__";
const HOSTS_REGEX = /^(?<protocol>[^/]+):\/\/(?:(?<username>[^:@]*)(?::(?<password>[^@]*))?@)?(?<hosts>(?!:)[^/?@]*)(?<rest>.*)/;

class URLWithoutHost extends whatwg_url_1.URL {
}
class MongoParseError extends Error {
    get name() {
        return 'MongoParseError';
    }
}
class ConnectionString extends URLWithoutHost {
    _hosts;
    constructor(uri, options = {}) {
        const { looseValidation } = options;
        const match = uri.match(HOSTS_REGEX);
        if (!match) {
            throw new MongoParseError('Invalid connection string');
        }
        const { protocol, username, password, hosts, rest } = match.groups ?? {};
        if (!looseValidation && (!protocol || !hosts)) {
            throw new MongoParseError('Protocol and host list are required in the uri');
        }
        let authString = '';
        if (typeof username === 'string')
            authString += username;
        if (typeof password === 'string')
            authString += `:${password}`;
        if (authString)
            authString += '@';
        try {
            super(`${protocol.toLowerCase()}://${authString}${DUMMY_HOSTNAME}${rest}`);
        }
        catch (err) {
            throw new MongoParseError(err.message);
        }
        this._hosts = hosts.split(',');
    }
    get hosts() {
        return this._hosts;
    }
}
exports.URLWithoutHost = URLWithoutHost;
exports.ConnectionString = ConnectionString;
exports.MongoParseError = MongoParseError;
exports.default = ConnectionString;
