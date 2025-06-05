#!/bin/sh
set -eu pipefail

# invok register --email user@example.com --password your_password

invok login --email user@example.com --password your_password

#invok create -n hello-world

invok deploy -n hello-world

invok list