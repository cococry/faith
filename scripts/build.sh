#!/bin/bash

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

build_project() {
    local project="$1"

    echo "Building $project..."

    (
        cd "$project" || exit 1
        cc -o nob nob.c || exit 1
        ./nob
    )

    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ $project built successfully${NC}"
    else
        echo -e "${RED}✗ $project build failed${NC}"
        return 1
    fi
}


build_project faith-proto
build_project faith-server
cd faith-server
./nob chain
cd ..
build_project faith-client
