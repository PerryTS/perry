import React, { useState } from "react";
import { render, Text, Box } from "ink";

function App() {
  const [count] = useState(0);
  return React.createElement(
    Box,
    null,
    React.createElement(Text, null, `count=${count}`),
  );
}

render(React.createElement(App));
