// worker.js
// This worker handles WASM initialization and OPFS-related operations.
// All file system and WASM operations are delegated to this worker.

import init, { install_deps, Opfs, Cwd } from './pkg/utoo_wasm.js';

let wasmReady = false;
let rootDir = null; // OPFS root directory handle

// Initialize the WASM module
async function initWasm() {
  if (!wasmReady) {
    await init();
    wasmReady = true;
  }
}

async function copyDirSync(root, srcPath, dstPath, options = {}) {
  const { overwrite = true, onProgress } = options;

  // Get source directory handle
  const srcDir = await getDirHandle(root, srcPath);
  if (!srcDir) {
    throw new Error(`Source directory not found: ${srcPath}`);
  }

  // Create destination directory
  const dstDir = await getDirHandle(root, dstPath, { create: true });

  // Count total files
  const totalFiles = await countFiles(srcDir);
  let processedFiles = 0;

  // Recursive copy function
  async function copyDirRecursive(srcHandle, dstHandle, relativePath = '') {
    for await (const [name, entry] of srcHandle.entries()) {
      const currentPath = relativePath ? `${relativePath}/${name}` : name;

      if (entry.kind === 'directory') {
        // Create subdirectory
        const newDstDir = await dstHandle.getDirectoryHandle(name, { create: true });
        // Recursively copy subdirectory
        await copyDirRecursive(entry, newDstDir, currentPath);
      } else if (entry.kind === 'file') {
        // Synchronously copy file
        await copyFileSync(entry, dstHandle, name, currentPath);
        processedFiles++;
        onProgress?.(processedFiles, totalFiles, currentPath);
      }
    }
  }

  // Use FileSystemSyncAccessHandle to synchronously copy files
  async function copyFileSync(srcFileHandle, dstDirHandle, fileName, filePath) {
    try {
      // Check if destination file exists
      if (!overwrite) {
        try {
          await dstDirHandle.getFileHandle(fileName);
          // File already exists, skip
          return;
        } catch {
          // File doesn't exist, continue copying
        }
      }

      // Get synchronous access handles
      const srcSyncHandle = await srcFileHandle.createSyncAccessHandle();
      const dstFileHandle = await dstDirHandle.getFileHandle(fileName, { create: true });
      const dstSyncHandle = await dstFileHandle.createSyncAccessHandle();

      try {
        // Get file size
        const fileSize = srcSyncHandle.getSize();

        // Use buffer for efficient copying
        const bufferSize = 64 * 1024; // 64KB buffer
        const buffer = new ArrayBuffer(bufferSize);
        const uint8Array = new Uint8Array(buffer);

        let offset = 0;
        while (offset < fileSize) {
          // Read data chunk
          const bytesRead = srcSyncHandle.read(uint8Array, { at: offset });
          if (bytesRead === 0) break;

          // Write data chunk
          dstSyncHandle.write(uint8Array.subarray(0, bytesRead), { at: offset });
          offset += bytesRead;
        }

        // Ensure data is written to disk
        dstSyncHandle.flush();

      } finally {
        // Close synchronous access handles
        srcSyncHandle.close();
        dstSyncHandle.close();
      }

    } catch (error) {
      console.error(`Failed to copy file: ${filePath}`, error);
      throw error;
    }
  }

  // Count total files
  async function countFiles(dirHandle) {
    let count = 0;
    for await (const [, entry] of dirHandle.entries()) {
      if (entry.kind === 'directory') {
        count += await countFiles(entry);
      } else {
        count++;
      }
    }
    return count;
  }

  // Get directory handle
  async function getDirHandle(root, path, { create = false } = {}) {
    if (!path) return root;
    const parts = path.split('/').filter(Boolean);
    let dir = root;
    for (const part of parts) {
      try {
        dir = await dir.getDirectoryHandle(part, { create });
      } catch (error) {
        if (!create) throw error;
        throw new Error(`Failed to access directory: ${path}`);
      }
    }
    return dir;
  }

  // Execute copy operation
  try {
    await copyDirRecursive(srcDir, dstDir);
    console.log(`Successfully copied ${processedFiles} files from ${srcPath} to ${dstPath}`);
  } catch (error) {
    console.error('Copy operation failed:', error);
    throw error;
  }
}


self.onmessage = async (event) => {
  const { type, payload } = event.data;

  await initWasm();

  if (type === 'install_deps') {
    const { lockContent, pkg } = payload;
    try {
      console.time('install_deps');
      const result = await install_deps(lockContent, pkg);
      await Cwd.set(`/${pkg}`);
      console.log('install_deps result', result);
      console.timeEnd('install_deps');
      self.postMessage({ type: 'install_deps_result', result });
    } catch (e) {
      self.postMessage({ type: 'error', error: e.message });
    }
  }
  // Handle fs.read
  else if (type === 'fs_read') {
    const { path } = payload;
    try {
      const result = await Opfs.read(path);
      console.log('fs_read result', result);
      self.postMessage({ type: 'fs_read_result', result });
    } catch (e) {
      self.postMessage({ type: 'error', error: e.message || e.toString() });
    }
  }
  else if (type === 'fs_read_dir') {
    const { path  } = payload;
    try {
      const result = await Opfs.read_dir(path);
      self.postMessage({ type: 'fs_read_dir_result', result });
    } catch (e) {
      self.postMessage({ type: 'error', error: e.message || e.toString() });
    }
  }
  // Handle fs.write
  else if (type === 'fs_write') {
    const { path, content } = payload;
    try {
      await Opfs.write(path, content);
      self.postMessage({ type: 'fs_write_result', result: 'success' });
    } catch (e) {
      self.postMessage({ type: 'error', error: e.message || e.toString() });
    }
  }
  // Handle fs.copy_dir
  else if (type === 'fs_copy_dir') {
    const { srcPath, dstPath } = payload;
    try {
      if (!rootDir) {
        rootDir = await navigator.storage.getDirectory();
      }
      console.time('copyDirSync');
      await copyDirSync(rootDir, srcPath, dstPath);
      console.timeEnd('copyDirSync');
      self.postMessage({ type: 'fs_copy_dir_result', result: 'success' });
    } catch (e) {
      self.postMessage({ type: 'error', error: e.message || e.toString() });
    }
  }

  else if (type === 'cwd_set') {
    const { path } = payload;
    try {
      await Cwd.set(path);
      self.postMessage({ type: 'cwd_set_result', result: 'success' });
    } catch (e) {
      self.postMessage({ type: 'error', error: e.message || e.toString() });
    }
  }
};
