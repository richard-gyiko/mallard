// JSX exercised by the TSX grammar.

export function Widget(props) {
  const label = formatLabel(props.name);
  return label;
}

function formatLabel(name) {
  return name.toUpperCase();
}
