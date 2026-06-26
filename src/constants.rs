//! Interned Python strings used across the driver.

use std::{ops::Deref, sync::Arc};

use pyo3::{Py, Python, sync::PyOnceLock, types::PyString};

pub(crate) struct ConstantsInner {
    /// The string `kosoku.cases`.
    pub(crate) kosoku_cases: Py<PyString>,

    /// The string `CASES`.
    pub(crate) cases: Py<PyString>,

    /// The string `onOpen`.
    pub(crate) on_open: Py<PyString>,
    /// The string `onConnectionLost`.
    pub(crate) on_connection_lost: Py<PyString>,
    /// The string `onPing`.
    pub(crate) on_ping: Py<PyString>,
    /// The string `onPong`.
    pub(crate) on_pong: Py<PyString>,
    /// The string `onClose`.
    pub(crate) on_close: Py<PyString>,
    /// The string `onMessage`.
    pub(crate) on_message: Py<PyString>,

    /// The string `DESCRIPTION`.
    pub(crate) description: Py<PyString>,
    /// The string `EXPECTATION`.
    pub(crate) expectation: Py<PyString>,
    /// The string `behavior`.
    pub(crate) behavior: Py<PyString>,
    /// The string `behaviorClose`.
    pub(crate) behavior_close: Py<PyString>,
    /// The string `result`.
    pub(crate) result: Py<PyString>,
    /// The string `resultClose`.
    pub(crate) result_close: Py<PyString>,
    /// The string `reportTime`.
    pub(crate) report_time: Py<PyString>,
    /// The string `reportCompressionRatio`.
    pub(crate) report_compression_ratio: Py<PyString>,
    /// The string `expected`.
    pub(crate) expected: Py<PyString>,
    /// The string `expectedClose`.
    pub(crate) expected_close: Py<PyString>,
    /// The string `received`.
    pub(crate) received: Py<PyString>,
    /// The string `trafficStats`.
    pub(crate) traffic_stats: Py<PyString>,

    // `TrafficStats.__json__` dict keys.
    /// The string `outgoingOctetsWireLevel`.
    pub(crate) outgoing_octets_wire_level: Py<PyString>,
    /// The string `outgoingOctetsWebSocketLevel`.
    pub(crate) outgoing_octets_websocket_level: Py<PyString>,
    /// The string `outgoingOctetsAppLevel`.
    pub(crate) outgoing_octets_app_level: Py<PyString>,
    /// The string `outgoingCompressionRatio`.
    pub(crate) outgoing_compression_ratio: Py<PyString>,
    /// The string `outgoingWebSocketOverhead`.
    pub(crate) outgoing_websocket_overhead: Py<PyString>,
    /// The string `outgoingWebSocketFrames`.
    pub(crate) outgoing_websocket_frames: Py<PyString>,
    /// The string `outgoingWebSocketMessages`.
    pub(crate) outgoing_websocket_messages: Py<PyString>,
    /// The string `preopenOutgoingOctetsWireLevel`.
    pub(crate) preopen_outgoing_octets_wire_level: Py<PyString>,
    /// The string `incomingOctetsWireLevel`.
    pub(crate) incoming_octets_wire_level: Py<PyString>,
    /// The string `incomingOctetsWebSocketLevel`.
    pub(crate) incoming_octets_websocket_level: Py<PyString>,
    /// The string `incomingOctetsAppLevel`.
    pub(crate) incoming_octets_app_level: Py<PyString>,
    /// The string `incomingCompressionRatio`.
    pub(crate) incoming_compression_ratio: Py<PyString>,
    /// The string `incomingWebSocketOverhead`.
    pub(crate) incoming_websocket_overhead: Py<PyString>,
    /// The string `incomingWebSocketFrames`.
    pub(crate) incoming_websocket_frames: Py<PyString>,
    /// The string `incomingWebSocketMessages`.
    pub(crate) incoming_websocket_messages: Py<PyString>,
    /// The string `preopenIncomingOctetsWireLevel`.
    pub(crate) preopen_incoming_octets_wire_level: Py<PyString>,

    // Wire-log entry tags (Autobahn's `FuzzingProtocol` log codes).
    /// The wire-log tag `WLM`.
    pub(crate) wirelog_mode: Py<PyString>,
    /// The wire-log tag `RO`.
    pub(crate) rx_octets: Py<PyString>,
    /// The wire-log tag `TO`.
    pub(crate) tx_octets: Py<PyString>,
    /// The wire-log tag `RF`.
    pub(crate) rx_frame: Py<PyString>,
    /// The wire-log tag `TF`.
    pub(crate) tx_frame: Py<PyString>,
    /// The wire-log tag `CT`.
    pub(crate) continue_scheduled: Py<PyString>,
    /// The wire-log tag `CTE`.
    pub(crate) continue_executed: Py<PyString>,
    /// The wire-log tag `KL`.
    pub(crate) kill_scheduled: Py<PyString>,
    /// The wire-log tag `KLE`.
    pub(crate) kill_executed: Py<PyString>,
    /// The wire-log tag `TI`.
    pub(crate) close_scheduled: Py<PyString>,
    /// The wire-log tag `TIE`.
    pub(crate) close_executed: Py<PyString>,
}

#[derive(Clone)]
pub(crate) struct Constants {
    inner: Arc<ConstantsInner>,
}

impl Deref for Constants {
    type Target = ConstantsInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Constants {
    pub(crate) fn get(py: Python<'_>) -> Constants {
        static CONSTANTS: PyOnceLock<Constants> = PyOnceLock::new();
        CONSTANTS.get_or_init(py, || Constants::new(py)).clone()
    }

    fn new(py: Python<'_>) -> Self {
        let s = |text: &str| PyString::new(py, text).unbind();
        let inner = ConstantsInner {
            kosoku_cases: s("kosoku.cases"),

            cases: s("CASES"),

            on_open: s("onOpen"),
            on_connection_lost: s("onConnectionLost"),
            on_ping: s("onPing"),
            on_pong: s("onPong"),
            on_close: s("onClose"),
            on_message: s("onMessage"),

            description: s("DESCRIPTION"),
            expectation: s("EXPECTATION"),
            behavior: s("behavior"),
            behavior_close: s("behaviorClose"),
            result: s("result"),
            result_close: s("resultClose"),
            report_time: s("reportTime"),
            report_compression_ratio: s("reportCompressionRatio"),
            expected: s("expected"),
            expected_close: s("expectedClose"),
            received: s("received"),
            traffic_stats: s("trafficStats"),

            outgoing_octets_wire_level: s("outgoingOctetsWireLevel"),
            outgoing_octets_websocket_level: s("outgoingOctetsWebSocketLevel"),
            outgoing_octets_app_level: s("outgoingOctetsAppLevel"),
            outgoing_compression_ratio: s("outgoingCompressionRatio"),
            outgoing_websocket_overhead: s("outgoingWebSocketOverhead"),
            outgoing_websocket_frames: s("outgoingWebSocketFrames"),
            outgoing_websocket_messages: s("outgoingWebSocketMessages"),
            preopen_outgoing_octets_wire_level: s("preopenOutgoingOctetsWireLevel"),
            incoming_octets_wire_level: s("incomingOctetsWireLevel"),
            incoming_octets_websocket_level: s("incomingOctetsWebSocketLevel"),
            incoming_octets_app_level: s("incomingOctetsAppLevel"),
            incoming_compression_ratio: s("incomingCompressionRatio"),
            incoming_websocket_overhead: s("incomingWebSocketOverhead"),
            incoming_websocket_frames: s("incomingWebSocketFrames"),
            incoming_websocket_messages: s("incomingWebSocketMessages"),
            preopen_incoming_octets_wire_level: s("preopenIncomingOctetsWireLevel"),

            wirelog_mode: s("WLM"),
            rx_octets: s("RO"),
            tx_octets: s("TO"),
            rx_frame: s("RF"),
            tx_frame: s("TF"),
            continue_scheduled: s("CT"),
            continue_executed: s("CTE"),
            kill_scheduled: s("KL"),
            kill_executed: s("KLE"),
            close_scheduled: s("TI"),
            close_executed: s("TIE"),
        };
        Self {
            inner: Arc::new(inner),
        }
    }
}
