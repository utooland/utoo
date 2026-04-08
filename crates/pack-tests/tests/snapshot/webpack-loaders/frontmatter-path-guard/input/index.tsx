// @ts-ignore
import markdownFrontmatter from './page.md?type=frontmatter';
// @ts-ignore
import jsFrontmatter from './page.js?type=frontmatter';

export default function App() {
    return (
        <div>
            <p id="markdown-frontmatter">{markdownFrontmatter}</p>
            <p id="js-frontmatter">{jsFrontmatter}</p>
        </div>
    );
}
