import "./style.css";
import "./index.less";
import "./index.sass";

document.body.innerHTML = `
  <div class="container">
    <div class="header">
      <h1>Inline CSS Demo</h1>
    </div>
    <div class="content">
      <p>This example demonstrates CSS inlining with LESS, SASS, and plain CSS.</p>
      <div class="grid">
        <div class="card">
          <h3>Plain CSS</h3>
          <p class="text-muted">Custom properties, grid layout, and responsive media queries.</p>
        </div>
        <div class="card">
          <h3>LESS</h3>
          <p class="text-muted">Variables, nesting, and mixins compiled to CSS then inlined.</p>
        </div>
        <div class="card">
          <h3>SASS</h3>
          <p class="text-muted">Indented syntax with variables, nesting, and built-in functions.</p>
        </div>
      </div>
      <div class="flex" style="margin-top: 16px;">
        <button class="btn btn--primary">Primary</button>
        <button class="btn btn--success">Success</button>
        <button class="btn btn--danger">Danger</button>
        <span class="badge badge--info">Info</span>
        <span class="badge badge--success">OK</span>
      </div>
    </div>
  </div>
`;
