mod DownloadHandler;
mod UploadHandlerRaw;

pub fn init() {
    DownloadHandler::init();
    UploadHandlerRaw::init();
}