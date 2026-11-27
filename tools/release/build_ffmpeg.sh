#!/bin/sh
# Build Studio's shared media libraries and command-line encoder from FFmpeg 8.0.3.
set -eu
prefix=$1
export MACOSX_DEPLOYMENT_TARGET=14.0
./configure --prefix="$prefix" --enable-shared --disable-static --disable-doc \
    --disable-debug --disable-autodetect --disable-everything --disable-ffplay \
    --enable-pic --enable-zlib --enable-avdevice --enable-indev=avfoundation \
    --enable-swscale --enable-swresample --enable-protocol=file,pipe \
    --enable-demuxer=mov,matroska,avi,image2,rawvideo \
    --enable-parser=h264,hevc,mpeg4video,mjpeg,png \
    --enable-decoder=h264,hevc,mpeg4,mjpeg,rawvideo,png \
    --enable-filter=null,anull,scale,format --enable-encoder=rawvideo,mpeg4 \
    --enable-muxer=null,rawvideo,mp4
make -j8
make install
