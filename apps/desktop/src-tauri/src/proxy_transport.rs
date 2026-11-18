//! Buffer ureq's HTTP CONNECT handshake before its first write to the proxy.
use std::fmt;
use ureq::config::Config;
use ureq::unversioned::{
    resolver::DefaultResolver,
    transport::{Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport},
};
use ureq::{Agent, Error, ProxyProtocol, http::Uri};

pub fn agent(config: Config) -> Agent {
    let Some(proxy) = config
        .proxy()
        .filter(|proxy| matches!(proxy.protocol(), ProxyProtocol::Http | ProxyProtocol::Https))
    else {
        return config.into();
    };
    let connector = ProxyConnector {
        uri: proxy.uri().clone(),
        inner: DefaultConnector::default(),
    };
    Agent::with_parts(config, connector, DefaultResolver::default())
}

struct ProxyConnector {
    uri: Uri,
    inner: DefaultConnector,
}

impl fmt::Debug for ProxyConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyConnector").finish_non_exhaustive()
    }
}

impl Connector for ProxyConnector {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<()>,
    ) -> Result<Option<Self::Out>, Error> {
        let transport = self.inner.connect(details, chained)?;
        // ureq removes the proxy from the config of the inner connection to the proxy itself.
        if details.config.proxy().is_none() && details.uri == &self.uri {
            return Ok(transport.map(|inner| {
                ConnectTransport {
                    inner,
                    pending: Some(Vec::new()),
                }
                .boxed()
            }));
        }
        Ok(transport)
    }
}

struct ConnectTransport {
    inner: Box<dyn Transport>,
    pending: Option<Vec<u8>>,
}

impl fmt::Debug for ConnectTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectTransport").finish_non_exhaustive()
    }
}

impl ConnectTransport {
    fn send_pending(&mut self, timeout: NextTimeout) -> Result<(), Error> {
        if let Some(pending) = self.pending.take().filter(|pending| !pending.is_empty()) {
            // Fall back to the underlying buffer size for unusually large proxy headers.
            let capacity = self.inner.buffers().output().len();
            for chunk in pending.chunks(capacity) {
                self.inner.buffers().output()[..chunk.len()].copy_from_slice(chunk);
                self.inner.transmit_output(chunk.len(), timeout)?;
            }
        }
        Ok(())
    }
}

impl Transport for ConnectTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        let Some(pending) = &mut self.pending else {
            return self.inner.transmit_output(amount, timeout);
        };
        let output = self.inner.buffers().output();
        pending.extend_from_slice(&output[..amount]);
        if pending.len() > output.len() {
            self.send_pending(timeout)?;
        }
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        // The first input read waits for the CONNECT response. Later TLS traffic passes through.
        self.send_pending(timeout)?;
        self.inner.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use ureq::unversioned::transport::{LazyBuffers, time::Duration};

    #[derive(Debug)]
    struct RecordingTransport {
        buffers: LazyBuffers,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Transport for RecordingTransport {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(&mut self, amount: usize, _: NextTimeout) -> Result<(), Error> {
            self.writes
                .lock()
                .unwrap()
                .push(self.buffers.output()[..amount].to_vec());
            Ok(())
        }

        fn await_input(&mut self, _: NextTimeout) -> Result<bool, Error> {
            assert!(!self.writes.lock().unwrap().is_empty());
            Ok(true)
        }

        fn is_open(&mut self) -> bool {
            true
        }
    }

    #[test]
    fn connect_fragments_are_written_together_and_tunnel_bytes_pass_through() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut transport = ConnectTransport {
            inner: Box::new(RecordingTransport {
                buffers: LazyBuffers::new(64, 64),
                writes: writes.clone(),
            }),
            pending: Some(Vec::new()),
        };
        let timeout = NextTimeout {
            after: Duration::NotHappening,
            reason: ureq::Timeout::Global,
        };
        let fragments: [&[u8]; 3] = [b"CONNECT ", b"example.org:443 HTTP/1.1\r\n", b"\r\n"];
        for part in fragments {
            transport.buffers().output()[..part.len()].copy_from_slice(part);
            transport.transmit_output(part.len(), timeout).unwrap();
            assert!(writes.lock().unwrap().is_empty());
        }
        transport.await_input(timeout).unwrap();
        assert_eq!(*writes.lock().unwrap(), [fragments.concat()]);

        transport.buffers().output()[..3].copy_from_slice(b"tls");
        transport.transmit_output(3, timeout).unwrap();
        assert_eq!(
            *writes.lock().unwrap(),
            [fragments.concat(), b"tls".to_vec()]
        );
    }
}
