// BENCHMARK-ONLY RSA-2048 key pair, generated for this harness and used for
// nothing else. It is committed on purpose so RS256 / RSA-PKCS#1 v1.5
// signatures are byte-identical across runs and arms. NOT A SECRET.
export const RSA_PRIVATE_PEM =
  "-----BEGIN RSA PRIVATE KEY-----\n" +
  "MIIEowIBAAKCAQEAvXiOJf+ECyr+oHE6l5GtqXNf384kXoCTD6M07DsmSkTdbqCK\n" +
  "Q3vMP7utUmEkuYY6xmZNliH0KG3R8C2CkVgrM1thVYcfrEWf+z8ke2bwI7azWPYw\n" +
  "XKOB03er4iTRMa0poZX1hSTSTsv83HrwOFFW/A4wOaS2iptjPt967Mbx/SIrFuxm\n" +
  "rOoaS2ATmUWfl/xvGQ60aFTHhOLXz3zNG5lAfdbn6iarD3hRD3PT7j1NYwVmUvr7\n" +
  "hoegKz3+l1LzgIUViPgRWj36D0RL8QJwsW0VL+0if0+KJi86xDAG2YUserEq4Lt+\n" +
  "zbE4OTloPug6HooGtzSe4geWXUROwWwwBz8rYQIDAQABAoIBABYDr+B9CRvoaaAS\n" +
  "spAcfmULNr82Sxt/15Z1jBdL4sMLr/7D6yIJfE3bbI6pzYPmM2YMn5nWEBxdG4t1\n" +
  "0483rgKll3BB5vgz5BwOgMVQdzkZvUo/hexS7dQNj4xoYrYMPKNoXRfp6Kr8deuC\n" +
  "4pkTOlwcT0DOATem86fBJ53jRQf4K4ijuEwnQZFiiImkXmg6IB/b3hhi4PhctkTs\n" +
  "CvYy3y7KWB9W1oEDfNLa8Kn0gZwWTlDmXguBPE3m/S+982LJ8rZun/TL4QB+N1Tu\n" +
  "Bu2t5IM5uaM5KCpWQDQmaQsHTqeXDxO49JjBuHePhpCblXw4Qy8RW578Om0DaixM\n" +
  "K57GFFECgYEA9gVrqODSw+zk7bnd/h63NdoEcYptbJ145XYUoFFE81JkNnRwoCBL\n" +
  "i3HwChZEZ6gSApPZ42c7Sf/+i+6J6xN4wouAxMxe1T2rGEiOf8yktiOKxzbL9Gc/\n" +
  "suzqKn/mIpS0zUY71lbBEUNpUYYk7TqAG6/mFKstc/MjwksjQjgDvOcCgYEAxSfw\n" +
  "3S31+VWDMl+H/kmxPe5anOnLbYg8rIAl+KCf4Ebf+FzHMZ+4s3jD/sC2keLZOGHH\n" +
  "bu6B0fY+Ij1d7brSInnQBMNDwxYHY7H0gY5rsALOUBb6K2+ltPHPv+HVwllD0WJ3\n" +
  "IWRNZLtcO3lxBP8wcYdEI7sjVRId+eQ1Yz5jRHcCgYBOcSwCjKynC8FyivDdNa30\n" +
  "3a7NBPYey5bgkuXAuCjj7EFHm5jNdX6g15NRpAfrhQs5BytR9nhQ/+6Jb2VKLssy\n" +
  "PIiyNveFxkPnWjsVRIrACFUXb8FYDBOjWSbQpjcaj4+WY+5wSPkGKBhMhhkACscO\n" +
  "DOevb2Tus3eTW6HCW1nVwQKBgQCdY5s3Fp/MYeWQaw8HgxDJXeRca4+Iaz/0fYDS\n" +
  "kHfQ9QOLI5WpGda6/2eHkZSttaivB+/LsP9V+/vyHYdEZuWlvBTCGJeZv5Y6ki+c\n" +
  "1XNGWZcV/KHN1x0z6+5rQgABXH11Q+PSdl4KUj/5AqOk14t2tgNBev1jxkjD2th7\n" +
  "16wrVQKBgGQ53RNV/nWtnE/5JJWxRZeNHJAKjUyou8SQ0RlTXEbXdujqZ2F/R5Dz\n" +
  "lGzrzjwuD1T1ETDOQM+8ce4CLs7X6XbjW6yoly7C9eaU3Z3n82GlIT3IeHyLqKor\n" +
  "21fbRAiU8XnSKsIg4FQ5vxADMW5T/fVM2HBhpLLS4zYZTYPTM/14\n" +
  "-----END RSA PRIVATE KEY-----\n";

export const RSA_PUBLIC_PEM =
  "-----BEGIN PUBLIC KEY-----\n" +
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAvXiOJf+ECyr+oHE6l5Gt\n" +
  "qXNf384kXoCTD6M07DsmSkTdbqCKQ3vMP7utUmEkuYY6xmZNliH0KG3R8C2CkVgr\n" +
  "M1thVYcfrEWf+z8ke2bwI7azWPYwXKOB03er4iTRMa0poZX1hSTSTsv83HrwOFFW\n" +
  "/A4wOaS2iptjPt967Mbx/SIrFuxmrOoaS2ATmUWfl/xvGQ60aFTHhOLXz3zNG5lA\n" +
  "fdbn6iarD3hRD3PT7j1NYwVmUvr7hoegKz3+l1LzgIUViPgRWj36D0RL8QJwsW0V\n" +
  "L+0if0+KJi86xDAG2YUserEq4Lt+zbE4OTloPug6HooGtzSe4geWXUROwWwwBz8r\n" +
  "YQIDAQAB\n" +
  "-----END PUBLIC KEY-----\n";
