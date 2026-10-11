// Presence work, independently of value reads (the final own value is undefined).
const n = Number(process.argv[2] || "2000000");
const width = Number(process.argv[3] || "3");
const mode = process.argv[4] || "own";
const o3: any = {key0: 0, key1: 1, key2: undefined};
const o10: any = {key0: 0, key1: 1, key2: 2, key3: 3, key4: 4, key5: 5, key6: 6, key7: 7, key8: 8, key9: undefined};
const o40: any = {key0: 0, key1: 1, key2: 2, key3: 3, key4: 4, key5: 5, key6: 6, key7: 7, key8: 8, key9: 9, key10: 10, key11: 11, key12: 12, key13: 13, key14: 14, key15: 15, key16: 16, key17: 17, key18: 18, key19: 19, key20: 20, key21: 21, key22: 22, key23: 23, key24: 24, key25: 25, key26: 26, key27: 27, key28: 28, key29: 29, key30: 30, key31: 31, key32: 32, key33: 33, key34: 34, key35: 35, key36: 36, key37: 37, key38: 38, key39: undefined};
const dictionary: any = {};
if (mode.startsWith("dictionary")) for (let i = 0; i < 1100; i++) dictionary["key" + i] = undefined;
const own = width === 3 ? o3 : width === 10 ? o10 : o40;
const base = mode.startsWith("dictionary") ? dictionary : own;
const child: any = Object.create(base);
const receiver: any = mode.endsWith("inherited") ? child : base;
const key = mode.endsWith("absent") ? "missing" : "key" + (mode.startsWith("dictionary") ? 1099 : width - 1);
let checksum = 0;
for (let i = 0; i < n; i++) { if (key in receiver) checksum++; }
console.log(checksum);
