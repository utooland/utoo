export const calls = [];
export let creations = 0;

export function createReference(data) {
  creations++;
  return class {
    async greet(input) {
      calls.push({ operation: data.operation, input });
      return `Hello ${input.name}`;
    }
  };
}
