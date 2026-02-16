use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use deno_core::{op2, JsBuffer, OpState, Resource, ResourceId};
use deno_error::JsErrorBox;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

struct TcpListenerResource {
    listener: TcpListener,
    local_addr: std::net::SocketAddr,
}

impl Resource for TcpListenerResource {
    fn name(&self) -> Cow<'_, str> {
        "tcpListener".into()
    }
}

struct TcpStreamResource {
    rd: RefCell<OwnedReadHalf>,
    wr: RefCell<OwnedWriteHalf>,
    local_addr: std::net::SocketAddr,
    peer_addr: std::net::SocketAddr,
    close_notify: tokio::sync::Notify,
}

impl Resource for TcpStreamResource {
    fn name(&self) -> Cow<'_, str> {
        "tcpStream".into()
    }
}

fn add_stream(
    state: &mut OpState,
    stream: TcpStream,
) -> Result<ResourceId, JsErrorBox> {
    let local_addr = stream
        .local_addr()
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let peer_addr = stream
        .peer_addr()
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let (rd, wr) = stream.into_split();
    Ok(state.resource_table.add(TcpStreamResource {
        rd: RefCell::new(rd),
        wr: RefCell::new(wr),
        local_addr,
        peer_addr,
        close_notify: tokio::sync::Notify::new(),
    }))
}

#[derive(Serialize)]
pub struct NetAddr {
    pub host: String,
    pub port: u16,
}

#[op2]
#[smi]
pub async fn op_net_listen(
    state: Rc<RefCell<OpState>>,
    #[string] host: String,
    #[smi] port: u16,
) -> Result<ResourceId, JsErrorBox> {
    let addr = format!("{}:{}", host, port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| JsErrorBox::generic(format!("Failed to bind {}: {}", addr, e)))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let rid = state
        .borrow_mut()
        .resource_table
        .add(TcpListenerResource {
            listener,
            local_addr,
        });
    Ok(rid)
}

#[op2]
#[smi]
pub async fn op_net_accept(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
) -> Result<ResourceId, JsErrorBox> {
    let listener_rc = state
        .borrow()
        .resource_table
        .get::<TcpListenerResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let (stream, _peer_addr) = listener_rc
        .listener
        .accept()
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let conn_rid = add_stream(&mut state.borrow_mut(), stream)?;
    Ok(conn_rid)
}

#[op2]
#[smi]
pub async fn op_net_connect(
    state: Rc<RefCell<OpState>>,
    #[string] host: String,
    #[smi] port: u16,
) -> Result<ResourceId, JsErrorBox> {
    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let rid = add_stream(&mut state.borrow_mut(), stream)?;
    Ok(rid)
}

#[op2]
#[serde]
pub async fn op_net_read(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[smi] len: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<TcpStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    let mut buf = vec![0u8; len as usize];
    let mut rd_ref = resource.rd.borrow_mut();
    tokio::select! {
        result = rd_ref.read(&mut buf) => {
            let n = result.map_err(|e| JsErrorBox::generic(e.to_string()))?;
            buf.truncate(n);
            Ok(buf)
        }
        _ = resource.close_notify.notified() => {
            // Connection was closed, return empty to signal EOF
            Ok(vec![])
        }
    }
}

#[op2]
#[smi]
pub async fn op_net_write(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
    #[buffer] data: JsBuffer,
) -> Result<u32, JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<TcpStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    resource
        .wr
        .borrow_mut()
        .write_all(data.as_ref())
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(data.len() as u32)
}

#[op2]
pub async fn op_net_shutdown(
    state: Rc<RefCell<OpState>>,
    #[smi] rid: ResourceId,
) -> Result<(), JsErrorBox> {
    let resource = state
        .borrow()
        .resource_table
        .get::<TcpStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;

    resource
        .wr
        .borrow_mut()
        .shutdown()
        .await
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(())
}

#[op2(fast)]
pub fn op_net_close(state: &mut OpState, #[smi] rid: ResourceId) -> Result<(), JsErrorBox> {
    if state
        .resource_table
        .take::<TcpListenerResource>(rid)
        .is_ok()
    {
        return Ok(());
    }
    // Signal any pending reads to abort before removing the resource
    if let Ok(resource) = state.resource_table.get::<TcpStreamResource>(rid) {
        resource.close_notify.notify_waiters();
    }
    state
        .resource_table
        .take::<TcpStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(())
}

#[op2]
#[serde]
pub fn op_net_local_addr(
    state: &mut OpState,
    #[smi] rid: ResourceId,
) -> Result<NetAddr, JsErrorBox> {
    if let Ok(listener) = state.resource_table.get::<TcpListenerResource>(rid) {
        return Ok(NetAddr {
            host: listener.local_addr.ip().to_string(),
            port: listener.local_addr.port(),
        });
    }
    if let Ok(stream) = state.resource_table.get::<TcpStreamResource>(rid) {
        return Ok(NetAddr {
            host: stream.local_addr.ip().to_string(),
            port: stream.local_addr.port(),
        });
    }
    Err(JsErrorBox::generic(format!("Bad resource ID: {}", rid)))
}

#[op2]
#[serde]
pub fn op_net_remote_addr(
    state: &mut OpState,
    #[smi] rid: ResourceId,
) -> Result<NetAddr, JsErrorBox> {
    let resource = state
        .resource_table
        .get::<TcpStreamResource>(rid)
        .map_err(|e| JsErrorBox::generic(e.to_string()))?;
    Ok(NetAddr {
        host: resource.peer_addr.ip().to_string(),
        port: resource.peer_addr.port(),
    })
}
