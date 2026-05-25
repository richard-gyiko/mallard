// JSX exercised by the TSX grammar.

export function Widget(props) {
  const label = formatLabel(props.name);
  return adornArrow(label);
}

function formatLabel(name) {
  return name.toUpperCase();
}

// Finding 8 — arrow function bound to const at module level should
// index as Function.
const adornArrow = (s) => {
  return "[" + s + "]";
};

// Finding 8 — named function_expression in expression position should
// also index. Mirrors axios PR #10901's `export default ... && function
// httpAdapter(c) {}` shape.
export default true && function namedFnExpr(input) {
  return input;
};
