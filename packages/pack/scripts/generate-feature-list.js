// markdown-generator.js

// Import the Node.js file system module and path module
const fs = require('fs');
const path = require('path');

/**
 * Helper function to format a camelCase or snake_case key into a human-readable title.
 * Examples: "featureLevel1" -> "Feature Level 1", "featureStatus" -> "Feature Status"
 * This function is primarily used for generating table headers and list item titles
 * from object keys.
 * @param {string} key - The key string to format.
 * @returns {string} The formatted title string.
 */
function formatKeyAsTitle(key) {
    // Replace camelCase with spaces and capitalize the first letter of each word
    return key
        .replace(/([A-Z])/g, ' $1') // Add space before capital letters
        .replace(/_/g, ' ') // Replace underscores with spaces
        .split(' ') // Split by spaces
        .map(word => word.charAt(0).toUpperCase() + word.slice(1)) // Capitalize each word
        .join(' ') // Join back with spaces
        .trim(); // Trim any leading/trailing spaces
}

/**
 * Generates a Markdown string from a given data object.
 * This function is tailored to process a specific nested JSON structure
 * where features are organized by levels with children.
 *
 * @param {object} data - The data object containing statusLegend and featuresStatusList.
 * @returns {string} The generated Markdown string.
 */
function generateMarkdownFromData(data) {
    let markdown = ``;

    // Check if data is an object and not null
    if (typeof data === 'object' && data !== null) {
        // If statusLegend exists, generate the status legend list first
        if (Array.isArray(data.statusLegend) && data.statusLegend.length > 0) {
            markdown += `### Feature Status Legend\n\n`; // Title for the legend section
            data.statusLegend.forEach(legendItem => {
                if (legendItem.symbol && legendItem.description) {
                    markdown += `* ${legendItem.symbol}: ${legendItem.description}\n`;
                }
            });
            markdown += `\n`; // Add a newline after the legend for spacing
        }

        // Process the main featuresStatusList array
        if (Array.isArray(data.featuresStatusList) && data.featuresStatusList.length > 0) {
            markdown += `## Features Status List\n\n`; // Main heading for the features table

            // Define the headers for the flattened table, including featureStatus
            const headers = [
                'featureLevel1',
                'featureLevel2',
                'featureStatus', // New header for the status
                'featureDetails',
                'remarks'
            ];

            // Create table header row, formatting each header
            markdown += '| ' + headers.map(formatKeyAsTitle).join(' | ') + ' |\n';
            // Create table separator row
            markdown += '|' + Array(headers.length).fill(' :-------------- ').join('|') + '|\n';

            let previousFeatureLevel1Name = null; // Variable to track the previous featureLevel1Name

            // Iterate through each featureLevel1 item
            data.featuresStatusList.forEach(level1Item => {
                const currentFeatureLevel1Name = level1Item.featureLevel1 || ''; // Get the current Level 1 name

                // Iterate through the children of each featureLevel1
                if (Array.isArray(level1Item.children)) {
                    level1Item.children.forEach((level2Item, index) => {
                        let displayFeatureLevel1Name = '';
                        // Only display featureLevel1Name if it's the first child of this level1Item
                        // AND it's different from the previous overall featureLevel1Name
                        if (index === 0 && currentFeatureLevel1Name !== previousFeatureLevel1Name) {
                            displayFeatureLevel1Name = currentFeatureLevel1Name;
                        }

                        const rowValues = [
                            displayFeatureLevel1Name,
                            level2Item.featureLevel2 || '',
                            level2Item.featureStatus || '', // Get the featureStatus
                            level2Item.featureDetails || '',
                            level2Item.remarks || ''
                        ];
                        markdown += '| ' + rowValues.join(' | ') + ' |\n';
                    });
                }
                // Update previousFeatureLevel1Name after processing all children for the current level1Item
                previousFeatureLevel1Name = currentFeatureLevel1Name;
            });
            markdown += '\n'; // Add a newline after the table for spacing
        } else {
            markdown += `_No features data provided or featuresStatusList is not an array._\n`;
        }
    } else {
        markdown += `_No data provided or data is not an object._\n`;
    }

    return markdown;
}

// --- Example Usage ---

let featuresData = {}; // Initialize as an empty object

try {
    // Construct the path to the JSON file
    const dataFilePath = path.resolve(__dirname, '../data/features-list.json');
    // Read the JSON file synchronously
    const jsonData = fs.readFileSync(dataFilePath, 'utf8');
    // Parse the JSON data
    featuresData = JSON.parse(jsonData);
    console.log(`Successfully loaded data from ${dataFilePath}`);
} catch (err) {
    console.error(`Error reading or parsing JSON file: ${err.message}`);
    // If there's an error, featuresData will remain an empty object,
    // or you could set a default structure for graceful degradation.
}

// 2. Generate Markdown
const generatedMarkdown = generateMarkdownFromData(featuresData);
const outputFileName = path.resolve(__dirname, '../data/features-list.md'); // Define the output file name

// 3. Print the Markdown to the console and write to a file
console.log("--- Generated Markdown for User Features List ---");
console.log(generatedMarkdown);

// Write the generated Markdown to a file synchronously
try {
    fs.writeFileSync(outputFileName, generatedMarkdown);
    console.log(`Markdown successfully written to ${outputFileName}`);
} catch (err) {
    console.error('Error writing Markdown to file:', err);
}

