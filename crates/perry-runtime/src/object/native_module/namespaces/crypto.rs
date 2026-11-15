//! Item-level native namespace declarations for `crypto` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_CRYPTO = "crypto" {
        names = ["node:crypto.default"];
        members = {
            argon2 = (ref("node:crypto.argon2"));
            argon2Sync = (ref("node:crypto.argon2Sync"));
            checkPrime = (ref("node:crypto.checkPrime"));
            checkPrimeSync = (ref("node:crypto.checkPrimeSync"));
            createCipheriv = (ref("node:crypto.createCipheriv"));
            createDecipheriv = (ref("node:crypto.createDecipheriv"));
            createDiffieHellman = (ref("node:crypto.createDiffieHellman"));
            createDiffieHellmanGroup = (ref("node:crypto.createDiffieHellmanGroup"));
            createECDH = (ref("node:crypto.createECDH"));
            createHash = (ref("node:crypto.createHash"));
            createHmac = (ref("node:crypto.createHmac"));
            createPrivateKey = (ref("node:crypto.createPrivateKey"));
            createPublicKey = (ref("node:crypto.createPublicKey"));
            createSecretKey = (ref("node:crypto.createSecretKey"));
            createSign = (ref("node:crypto.createSign"));
            createVerify = (ref("node:crypto.createVerify"));
            diffieHellman = (ref("node:crypto.diffieHellman"));
            generatePrime = (ref("node:crypto.generatePrime"));
            generatePrimeSync = (ref("node:crypto.generatePrimeSync"));
            getCiphers = (ref("node:crypto.getCiphers"));
            getCipherInfo = (ref("node:crypto.getCipherInfo"));
            getCurves = (ref("node:crypto.getCurves"));
            getDiffieHellman = (ref("node:crypto.getDiffieHellman"));
            getHashes = (ref("node:crypto.getHashes"));
            hkdf = (ref("node:crypto.hkdf"));
            hkdfSync = (ref("node:crypto.hkdfSync"));
            pbkdf2 = (ref("node:crypto.pbkdf2"));
            pbkdf2Sync = (ref("node:crypto.pbkdf2Sync"));
            generateKeyPair = (ref("node:crypto.generateKeyPair"));
            generateKeyPairSync = (ref("node:crypto.generateKeyPairSync"));
            generateKey = (ref("node:crypto.generateKey"));
            generateKeySync = (ref("node:crypto.generateKeySync"));
            privateDecrypt = (ref("node:crypto.privateDecrypt"));
            privateEncrypt = (ref("node:crypto.privateEncrypt"));
            publicDecrypt = (ref("node:crypto.publicDecrypt"));
            publicEncrypt = (ref("node:crypto.publicEncrypt"));
            randomBytes = (ref("node:crypto.randomBytes"));
            randomFill = (ref("node:crypto.randomFill"));
            randomFillSync = (ref("node:crypto.randomFillSync"));
            randomInt = (ref("node:crypto.randomInt"));
            randomUUID = (ref("node:crypto.randomUUID"));
            randomUUIDv7;
            scrypt = (ref("node:crypto.scrypt"));
            scryptSync = (ref("node:crypto.scryptSync"));
            sign = (ref("node:crypto.sign"));
            setEngine = (undefined);
            timingSafeEqual = (ref("node:crypto.timingSafeEqual"));
            getFips = (ref("node:crypto.getFips"));
            setFips = (ref("node:crypto.setFips"));
            verify = (ref("node:crypto.verify"));
            hash = (ref("node:crypto.hash"));
            encapsulate = (ref("node:crypto.encapsulate"));
            decapsulate = (ref("node:crypto.decapsulate"));
            Certificate = (ref("node:crypto.Certificate"));
            Cipheriv = (ref("node:crypto.Cipheriv"));
            Decipheriv = (ref("node:crypto.Decipheriv"));
            DiffieHellman = (ref("node:crypto.DiffieHellman"));
            DiffieHellmanGroup = (ref("node:crypto.DiffieHellmanGroup"));
            ECDH = (ref("node:crypto.ECDH"));
            Hash = (ref("node:crypto.Hash"));
            Hmac = (ref("node:crypto.Hmac"));
            KeyObject = (ref("node:crypto.KeyObject"));
            Sign = (ref("node:crypto.Sign"));
            Verify = (ref("node:crypto.Verify"));
            X509Certificate = (ref("node:crypto.X509Certificate"));
            secureHeapUsed = (ref("node:crypto.secureHeapUsed"));
            constants;
            webcrypto = (ref("node:crypto.webcrypto"));
            subtle = (ref("node:crypto.subtle"));
            getRandomValues = (ref("node:crypto.getRandomValues"));
        }
    }
}

crate::realm_decl! {
    namespace NM_CRYPTO_CONSTANTS = "crypto.constants" {
        cell = "node:crypto.constants";
        names = ["node:crypto.default.constants"];
        members = {
            OPENSSL_VERSION_NUMBER = (811597840.0);
            SSL_OP_ALL = (2147485776.0);
            SSL_OP_ALLOW_NO_DHE_KEX = (1024.0);
            SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION = (262144.0);
            SSL_OP_CIPHER_SERVER_PREFERENCE = (4194304.0);
            SSL_OP_CISCO_ANYCONNECT = (32768.0);
            SSL_OP_COOKIE_EXCHANGE = (8192.0);
            SSL_OP_CRYPTOPRO_TLSEXT_BUG = (2147483648.0);
            SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS = (2048.0);
            SSL_OP_LEGACY_SERVER_CONNECT = (4.0);
            SSL_OP_NO_COMPRESSION = (131072.0);
            SSL_OP_NO_ENCRYPT_THEN_MAC = (524288.0);
            SSL_OP_NO_QUERY_MTU = (4096.0);
            SSL_OP_NO_RENEGOTIATION = (1073741824.0);
            SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION = (65536.0);
            SSL_OP_NO_SSLv2 = (0.0);
            SSL_OP_NO_SSLv3 = (33554432.0);
            SSL_OP_NO_TICKET = (16384.0);
            SSL_OP_NO_TLSv1 = (67108864.0);
            SSL_OP_NO_TLSv1_1 = (268435456.0);
            SSL_OP_NO_TLSv1_2 = (134217728.0);
            SSL_OP_NO_TLSv1_3 = (536870912.0);
            SSL_OP_PRIORITIZE_CHACHA = (2097152.0);
            SSL_OP_TLS_ROLLBACK_BUG = (8388608.0);
            ENGINE_METHOD_RSA = (1.0);
            ENGINE_METHOD_DSA = (2.0);
            ENGINE_METHOD_DH = (4.0);
            ENGINE_METHOD_RAND = (8.0);
            ENGINE_METHOD_EC = (2048.0);
            ENGINE_METHOD_CIPHERS = (64.0);
            ENGINE_METHOD_DIGESTS = (128.0);
            ENGINE_METHOD_PKEY_METHS = (512.0);
            ENGINE_METHOD_PKEY_ASN1_METHS = (1024.0);
            ENGINE_METHOD_ALL = (65535.0);
            ENGINE_METHOD_NONE = (0.0);
            DH_CHECK_P_NOT_SAFE_PRIME = (2.0);
            DH_CHECK_P_NOT_PRIME = (1.0);
            DH_UNABLE_TO_CHECK_GENERATOR = (4.0);
            DH_NOT_SUITABLE_GENERATOR = (8.0);
            RSA_PKCS1_PADDING = (1.0);
            RSA_NO_PADDING = (3.0);
            RSA_PKCS1_OAEP_PADDING = (4.0);
            RSA_X931_PADDING = (5.0);
            RSA_PKCS1_PSS_PADDING = (6.0);
            RSA_PSS_SALTLEN_DIGEST = (-1.0);
            RSA_PSS_SALTLEN_MAX_SIGN = (-2.0);
            RSA_PSS_SALTLEN_AUTO = (-2.0);
            TLS1_VERSION = (769.0);
            TLS1_1_VERSION = (770.0);
            TLS1_2_VERSION = (771.0);
            TLS1_3_VERSION = (772.0);
            POINT_CONVERSION_COMPRESSED = (2.0);
            POINT_CONVERSION_UNCOMPRESSED = (4.0);
            POINT_CONVERSION_HYBRID = (6.0);
        }
    }
}

crate::realm_decl! {
    namespace NM_CRYPTO_SUBTLE = "crypto.subtle" {
        cell = "node:crypto.subtle";
        extends = (SubtleCrypto.prototype);
        members = {
        }
    }
}

crate::realm_decl! {
    namespace NM_CRYPTO_WEBCRYPTO = "crypto.webcrypto" {
        cell = "node:crypto.webcrypto";
        extends = (Crypto.prototype);
        members = {
        }
    }
}
