"""Check real local port ownership without starting any development servers."""
import socket
import sys
import unittest
from unittest.mock import patch

import launcher


class DevPortTests(unittest.TestCase):
	def probe(self, host, port):
		action = launcher.Action("fixture", "Port fixture", "", browser_url=f"http://{host}:{port}")
		with patch.object(launcher, "action_for", return_value=action):
			launcher.ensure_dev_port_free("fixture")

	def reserve(self, family=socket.AF_INET, host="127.0.0.1"):
		reservation = socket.socket(family, socket.SOCK_STREAM)
		self.addCleanup(reservation.close)
		if family == socket.AF_INET6:
			reservation.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
		if sys.platform == "win32":
			reservation.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
		reservation.bind((host, 0))
		return reservation

	def test_free_port_needs_no_connection_and_is_released_after_check(self):
		reservation = self.reserve()
		port = reservation.getsockname()[1]
		reservation.close()
		with patch.object(launcher.socket, "create_connection", side_effect=TimeoutError("timed out")) as connect:
			self.probe("127.0.0.1", port)
			connect.assert_not_called()
		# The check must not leave the port occupied when Angular starts.
		with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
			server.bind(("127.0.0.1", port))
			server.listen()

	def test_listening_server_is_rejected(self):
		server = self.reserve()
		server.listen()
		with self.assertRaisesRegex(RuntimeError, "already in use"):
			self.probe("127.0.0.1", server.getsockname()[1])

	def test_reserved_port_without_a_listener_is_rejected(self):
		reservation = self.reserve()
		with self.assertRaisesRegex(RuntimeError, "already in use"):
			self.probe("127.0.0.1", reservation.getsockname()[1])

	@unittest.skipUnless(socket.has_ipv6, "IPv6 localhost is unavailable")
	def test_localhost_checks_the_ipv6_address_too(self):
		try:
			server = self.reserve(socket.AF_INET6, "::1")
		except OSError as error:
			self.skipTest(f"IPv6 localhost is unavailable: {error}")
		server.listen()
		with self.assertRaisesRegex(RuntimeError, "already in use"):
			self.probe("localhost", server.getsockname()[1])


if __name__ == "__main__":
	unittest.main()
