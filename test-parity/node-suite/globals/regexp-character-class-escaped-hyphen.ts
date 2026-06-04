const patterns = [
  "[\\-]",
  "[a\\- ]",
  "[a-z]",
  "[:]",
  "[ ]",
  "[a-z ]",
  "[:\\- ]",
  " {0,3}\\|?(?:[:\\- ]*\\|)+[\\:\\- ]*\\n",
];

for (const pattern of patterns) {
  try {
    const regex = new RegExp(pattern);
    console.log("OK  /" + pattern + "/ " + regex.test("-"));
  } catch (error: any) {
    console.log("FAIL /" + pattern + "/ -> " + error.message);
  }
}
