use crate::{TcpSendWorker, TCP};
use core::ops::Deref;
use ockam_core::{async_trait, Any};
use ockam_core::{Address, Decodable, LocalMessage, Result, Routed, Worker};
use ockam_node::Context;
use ockam_transport_core::TransportError;
use std::collections::BTreeMap;
use tracing::{debug, trace};

mod handle;
mod messages;

pub(crate) use handle::*;
pub(crate) use messages::*;

/// A TCP address router and connection listener
///
/// In order to create new TCP connection workers you need a router to
/// map remote addresses of `type = 1` to worker addresses.  This type
/// facilitates this.
///
/// Optionally you can also start listening for incoming connections
/// if the local node is part of a server architecture.
pub(crate) struct TcpRouter {
    ctx: Context,
    main_addr: Address,
    api_addr: Address,
    map: BTreeMap<Address, Address>,
    allow_auto_connection: bool,
}

impl TcpRouter {
    async fn create_self_handle(&self) -> Result<TcpRouterHandle> {
        let handle_ctx = self.ctx.new_context(Address::random(0)).await?;
        let handle = TcpRouterHandle::new(handle_ctx, self.api_addr.clone());
        Ok(handle)
    }

    async fn handle_register(&mut self, accepts: Vec<Address>, self_addr: Address) -> Result<()> {
        if let Some(f) = accepts.first().cloned() {
            trace!("TCP registration request: {} => {}", f, self_addr);
        } else {
            // Should not happen
            return Err(TransportError::InvalidAddress.into());
        }

        for accept in &accepts {
            if self.map.contains_key(accept) {
                return Err(TransportError::AlreadyConnected.into());
            }
        }
        for accept in accepts {
            self.map.insert(accept.clone(), self_addr.clone());
        }

        Ok(())
    }

    async fn handle_unregister(&mut self, self_addr: Address) -> Result<()> {
        trace!("TCP unregistration request: {}", &self_addr);

        self.map.retain(|_, self_addr_i| self_addr_i != &self_addr);

        Ok(())
    }

    async fn handle_connect(&mut self, peer: String) -> Result<Address> {
        let (peer_addr, hostnames) = TcpRouterHandle::resolve_peer(peer)?;

        let router_handle = self.create_self_handle().await?;
        let pair =
            TcpSendWorker::start_pair(&self.ctx, router_handle, None, peer_addr, hostnames).await?;

        let tcp_address: Address = format!("{}#{}", TCP, pair.peer()).into();
        let mut accepts = vec![tcp_address];
        accepts.extend(
            pair.hostnames()
                .iter()
                .map(|x| Address::from_string(format!("{}#{}", TCP, x))),
        );
        let self_addr = pair.tx_addr();

        self.handle_register(accepts, self_addr.clone()).await?;

        Ok(self_addr)
    }

    async fn handle_disconnect(&mut self, peer: String) -> Result<()> {
        let (peer_addr, _hostnames) = TcpRouterHandle::resolve_peer(peer)?;
        let tcp_address: Address = format!("{}#{}", TCP, peer_addr).into();

        let self_address = if let Some(self_address) = self.map.get(&tcp_address) {
            self_address.clone()
        } else {
            return Err(TransportError::PeerNotFound.into());
        };

        self.handle_unregister(self_address.clone()).await?;

        self.ctx.stop_worker(self_address).await?;

        Ok(())
    }

    async fn handle_route(&mut self, ctx: &Context, mut msg: LocalMessage) -> Result<()> {
        trace!(
            "TCP route request: {:?}",
            msg.transport().onward_route.next()
        );

        // Get the next hop
        let onward = msg.transport().onward_route.next()?;

        let next;
        // Look up the connection worker responsible
        if let Some(n) = self.map.get(onward) {
            // Connection already exists
            next = n.clone();
        } else {
            // No existing connection
            let peer_str;
            if let Ok(s) = String::from_utf8(onward.deref().clone()) {
                peer_str = s;
            } else {
                return Err(TransportError::UnknownRoute.into());
            }

            // TODO: Check if this is the hostname and we have existing/pending connection to this IP
            if self.allow_auto_connection {
                next = self.handle_connect(peer_str).await?;
            } else {
                return Err(TransportError::UnknownRoute.into());
            }
        }

        let _ = msg.transport_mut().onward_route.step()?;
        // Modify the transport message route
        msg.transport_mut()
            .onward_route
            .modify()
            .prepend(next.clone());

        // Send the transport message to the connection worker
        ctx.send(next.clone(), msg).await?;

        Ok(())
    }
}

#[async_trait]
impl Worker for TcpRouter {
    type Context = Context;
    type Message = Any;

    async fn initialize(&mut self, ctx: &mut Context) -> Result<()> {
        ctx.set_cluster(crate::CLUSTER_NAME).await?;
        Ok(())
    }

    async fn handle_message(&mut self, ctx: &mut Context, msg: Routed<Any>) -> Result<()> {
        let return_route = msg.return_route();
        let msg_addr = msg.msg_addr();

        if msg_addr == self.main_addr {
            let msg = LocalMessage::decode(msg.payload())?;
            self.handle_route(ctx, msg).await?;
        } else if msg_addr == self.api_addr {
            let msg = TcpRouterRequest::decode(msg.payload())?;
            match msg {
                TcpRouterRequest::Register { accepts, self_addr } => {
                    let res = self.handle_register(accepts, self_addr).await;

                    ctx.send(return_route, TcpRouterResponse::Register(res))
                        .await?;
                }
                TcpRouterRequest::Unregister { self_addr } => {
                    let res = self.handle_unregister(self_addr).await;

                    ctx.send(return_route, TcpRouterResponse::Unregister(res))
                        .await?;
                }
                TcpRouterRequest::Connect { peer } => {
                    let res = self.handle_connect(peer).await;

                    ctx.send(return_route, TcpRouterResponse::Connect(res))
                        .await?;
                }
                TcpRouterRequest::Disconnect { peer } => {
                    let res = self.handle_disconnect(peer).await;

                    ctx.send(return_route, TcpRouterResponse::Disconnect(res))
                        .await?;
                }
            };
        } else {
            return Err(TransportError::InvalidAddress.into());
        }

        Ok(())
    }
}

impl TcpRouter {
    /// Create and register a new TCP router with the node context
    pub async fn register(ctx: &Context) -> Result<TcpRouterHandle> {
        let main_addr = Address::random(0);
        let api_addr = Address::random(0);
        debug!("Initialising new TcpRouter with address {}", &main_addr);

        let child_ctx = ctx.new_context(Address::random(0)).await?;

        let router = Self {
            ctx: child_ctx,
            main_addr: main_addr.clone(),
            api_addr: api_addr.clone(),
            map: BTreeMap::new(),
            allow_auto_connection: true,
        };

        let handle = router.create_self_handle().await?;

        ctx.start_worker(vec![main_addr.clone(), api_addr], router)
            .await?;
        trace!("Registering TCP router for type = {}", TCP);
        ctx.register(TCP, main_addr).await?;

        Ok(handle)
    }
}
