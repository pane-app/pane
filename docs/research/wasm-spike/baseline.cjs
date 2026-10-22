// Equivalent toy query for process-startup context, not a launcher benchmark.
const items = [
  { id: "apps", title: "Open Applications" },
  { id: "calculator", title: "Calculator" },
  { id: "links", title: "Quicklinks" },
];
console.log(JSON.stringify(items.filter(item => item.title.toLowerCase().includes("calc"))));
