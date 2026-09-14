package dev.loopy.keybridge.remote;

interface IRemoteInputOwnership {
    boolean acquire();
    void release();
}
