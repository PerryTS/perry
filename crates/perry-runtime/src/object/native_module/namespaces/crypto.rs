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
