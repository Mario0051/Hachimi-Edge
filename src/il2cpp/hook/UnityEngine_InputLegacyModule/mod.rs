pub mod Input;

pub fn init() {
    get_assembly_image_or_return!(image, "UnityEngine.InputLegacyModule.dll");

    Input::init(image);
}
