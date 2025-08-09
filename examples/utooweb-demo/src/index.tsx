import React, { useState, useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import "./styles.css"

import { Project } from "@utoo/web";
import { packageLock } from "./packageLock";

async function initOpfsProject() {
  const project = new Project("utooweb-demo");

  console.log(
    "%cOPFS Project:%c Start to install dependencies.",
    "color: blue;",
    "color: green;",
  );

  const start = performance.now();
  await project.install(JSON.stringify(packageLock));

  console.log(
    `%cOPFS Project:%c Finished to install dependencies in ${Math.round(performance.now() - start)} ms.`,
    "color: blue;",
    "color: green;",
  );

}

const App = () => {
  const [rootDirectoryHandle, setRootDirectoryHandle] = useState(null);
  const [fileTree, setFileTree] = useState([]);
  const [selectedFilePath, setSelectedFilePath] = useState('');
  const [selectedFileContent, setSelectedFileContent] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState('');
  
  const FileTreeItem = ({ item }) => {
    const [isCollapsed, setIsCollapsed] = useState(true);

    const toggleCollapse = (e) => {
      e.preventDefault();
      if (item.type === 'directory') {
        setIsCollapsed(!isCollapsed);
      } else {
        fetchFileContent(item.path);
      }
    };

    return (
      <li style={{listStyleType: 'none'}}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '0.5rem',
            padding: '0.25rem 0.5rem',
            borderRadius: '0.5rem',
            cursor: 'pointer',
            transition: 'background-color 0.2s ease-in-out'
          }}
          onClick={toggleCollapse}
        >
          <span style={{width: '1rem', textAlign: 'center'}}>{item.type === 'directory' ? (isCollapsed ? '▶' : '▼') : '📄'}</span>
          <span>{item.name}</span>
        </div>
        {item.type === 'directory' && !isCollapsed && (
          <ul style={{
            paddingLeft: '1rem',
            borderLeft: '1px solid #d1d5db',
            marginLeft: '0.5rem',
            marginTop: '0.25rem'
          }}>
            {item.children.map((child, index) => (
              <FileTreeItem key={index} item={child} />
            ))}
          </ul>
        )}
      </li>
    );
  };

  const readDirectoryRecursive = async (directoryHandle, path = '') => {
    const tree = [];
    for await (const [name, handle] of directoryHandle.entries()) {
      const newPath = `${path}/${name}`;
      if (handle.kind === 'directory') {
        const children = await readDirectoryRecursive(handle, newPath);
        tree.push({ name, type: 'directory', path: newPath, children });
      } else {
        tree.push({ name, type: 'file', path: newPath });
      }
    }
    return tree;
  };

  const getDirectoryHandleByPath = async (directoryHandle, path) => {
    const pathSegments = path.split('/').filter(Boolean);
    let currentHandle = directoryHandle;
    for (const segment of pathSegments) {
      if (currentHandle.kind !== 'directory') {
        throw new Error('Invalid path. A segment is not a directory.');
      }
      currentHandle = await currentHandle.getDirectoryHandle(segment);
    }
    return currentHandle;
  };

  const fetchFileContent = async (filePath) => {
    setSelectedFilePath(filePath);
    setSelectedFileContent('');
    try {
      if (!rootDirectoryHandle) throw new Error("Root directory not initialized.");
      
      const parentDirPath = filePath.substring(0, filePath.lastIndexOf('/'));
      const fileName = filePath.substring(filePath.lastIndexOf('/') + 1);

      const parentDirHandle = await getDirectoryHandleByPath(rootDirectoryHandle, parentDirPath);
      const fileHandle = await parentDirHandle.getFileHandle(fileName);
      
      const file = await fileHandle.getFile();
      const content = await file.text();
      setSelectedFileContent(content);
    } catch (e) {
      setError(`Error reading file: ${e.message}`);
    }
  };

  useEffect(() => {
    const initialize = async () => {
      try {
        if (!navigator.storage || !navigator.storage.getDirectory) {
          throw new Error('Your browser does not support the Origin Private File System.');
        }

        await initOpfsProject();
        
        const rootHandle = await navigator.storage.getDirectory();
        setRootDirectoryHandle(rootHandle);

        const tree = await readDirectoryRecursive(rootHandle);
        setFileTree(tree);
      } catch (e) {
        setError(`Initialization failed: ${e.message}`);
      } finally {
        setIsLoading(false);
      }
    };

    initialize();
  }, []);

  return (
    <div style={{
      height: '100vh',
      padding: '1rem',
      display: 'flex',
      flexDirection: 'row',
      gap: '1rem',
      backgroundColor: '#f3f4f6',
      fontFamily: 'sans-serif'
    }}>
      <div style={{
        width: '30%',
        padding: '1.5rem',
        backgroundColor: '#ffffff',
        borderRadius: '0.75rem',
        boxShadow: '0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06)',
        overflowY: 'auto'
      }}>
        <h2 style={{
          fontSize: '1.5rem',
          fontWeight: '700',
          marginBottom: '1rem',
          paddingBottom: '0.5rem',
          borderBottom: '2px solid #e5e7eb',
          color: '#374151'
        }}>Project Explorer</h2>
        {isLoading && <p style={{textAlign: 'center', color: '#6b7280'}}>Loading file system...</p>}
        {error && <p style={{textAlign: 'center', color: '#ef4444'}}>{error}</p>}
        {!isLoading && !error && (
          <ul style={{
            display: 'flex',
            flexDirection: 'column',
            gap: '0.25rem',
            padding: 0
          }}>
            {fileTree.map((item, index) => (
              <FileTreeItem key={index} item={item} />
            ))}
          </ul>
        )}
      </div>

      <div style={{
        width: '70%',
        padding: '1.5rem',
        backgroundColor: '#ffffff',
        borderRadius: '0.75rem',
        boxShadow: '0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06)',
        overflowY: 'auto',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        alignItems: 'center'
      }}>
        {!selectedFilePath && (
          <p style={{textAlign: 'center', color: '#9ca3af', margin: 'auto'}}>Click a file on the left to view its content.</p>
        )}
        {selectedFilePath && (
          <>
            <h3 style={{fontSize: '1.25rem', fontWeight: '700', marginBottom: '0.5rem'}}>File Path: {selectedFilePath}</h3>
            <textarea
              id="code-mirror-editor"
              value={selectedFileContent}
              style={{
                flex: '1',
                minHeight: '300px',
                backgroundColor: '#f3f4f6',
                padding: '1rem',
                borderRadius: '0.5rem',
                fontSize: '0.875rem',
                width: '100%',
                boxSizing: 'border-box',
                resize: 'none'
              }}
              readOnly
            />
          </>
        )}
      </div>
    </div>
  );
};


ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
