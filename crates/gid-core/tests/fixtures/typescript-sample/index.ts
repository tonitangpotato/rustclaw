// Main entry point demonstrating call edges
import { greet, add, Calculator } from "./utils";

function main() {
    // Call to greet - should resolve to utils.ts:greet
    const message = greet("World");
    console.log(message);

    // Call to add - should resolve to utils.ts:add
    const sum = add(5, 3);
    console.log(`Sum: ${sum}`);

    // Method calls on Calculator instance
    const calc = new Calculator();
    const product = calc.multiply(4, 7);
    const quotient = calc.divide(10, 2);
    
    console.log(`Product: ${product}, Quotient: ${quotient}`);
}

main();
